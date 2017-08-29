extern crate env_logger;
extern crate tftp_server;

#[macro_use]
extern crate log;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, IpAddr, Ipv4Addr, UdpSocket};
use std::thread;
use std::time::Duration;
use tftp_server::packet::{ErrorCode, Packet, MAX_PACKET_SIZE};
use tftp_server::server::{Result, TftpServer};

trait Read512 {
    fn read_512(&mut self, buf: &mut Vec<u8>) -> io::Result<usize>;
}

impl<T> Read512 for T
where
    T: Read,
{
    fn read_512(&mut self, mut buf: &mut Vec<u8>) -> io::Result<usize> {
        self.take(512).read_to_end(&mut buf)
    }
}

const TIMEOUT: u64 = 3;

fn create_socket(timeout: Option<Duration>) -> Result<UdpSocket> {
    let socket = UdpSocket::bind((IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0))?;
    socket.set_nonblocking(false)?;
    socket.set_read_timeout(timeout)?;
    socket.set_write_timeout(timeout)?;
    Ok(socket)
}

/// Starts the server in a new thread.
pub fn start_server() -> Result<Vec<SocketAddr>> {
    let mut server = TftpServer::new()?;
    let mut addrs = vec![];
    server.get_local_addrs(&mut addrs)?;
    thread::spawn(move || {
        if let Err(e) = server.run() {
            println!("Error with server: {:?}", e);
        }
        ()
    });

    Ok(addrs)
}

pub fn assert_files_identical(fa: &str, fb: &str) {
    assert!(fs::metadata(fa).is_ok());
    assert!(fs::metadata(fb).is_ok());

    let (mut f1, mut f2) = (File::open(fa).unwrap(), File::open(fb).unwrap());
    let mut buf1 = String::new();
    let mut buf2 = String::new();

    f1.read_to_string(&mut buf1).unwrap();
    f2.read_to_string(&mut buf2).unwrap();

    assert_eq!(buf1, buf2);
}

fn timeout_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(None)?;
    let init_packet = Packet::WRQ {
        filename: "hello.txt".into(),
        mode: "octet".into(),
    };
    socket.send_to(
        init_packet.into_bytes()?.to_slice(),
        server_addr,
    )?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let reply_packet = Packet::read(&buf[0..amt])?;
    assert_eq!(reply_packet, Packet::ACK(0));


    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let reply_packet = Packet::read(&buf[0..amt])?;
    assert_eq!(reply_packet, Packet::ACK(0));

    assert!(fs::metadata("./hello.txt").is_ok());
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

struct WritingTransfer {
    socket: UdpSocket,
    file: File,
    block_num: u16,
    remote: Option<SocketAddr>,
}

impl WritingTransfer {
    fn start(local_file: &str, server_addr: &SocketAddr, server_file: &str) -> Self {
        let xfer = Self {
            socket: create_socket(Some(Duration::from_secs(TIMEOUT))).unwrap(),
            file: File::open(local_file).expect(&format!("cannot open {}", local_file)),
            block_num: 0,
            remote: None,
        };
        let init_packet = Packet::WRQ {
            filename: server_file.into(),
            mode: "octet".into(),
        };
        xfer.socket
            .send_to(init_packet.to_bytes().unwrap().to_slice(), &server_addr)
            .expect(&format!(
                "cannot send initial packet {:?} to {:?}",
                init_packet,
                server_addr
            ));
        xfer
    }

    fn step(&mut self, rx_buf: &mut [u8; MAX_PACKET_SIZE]) -> Option<()> {
        let (amt, src) = self.socket.recv_from(rx_buf).expect("cannot receive");
        if self.remote.is_some() {
            assert_eq!(self.remote.unwrap(), src, "transfer source changed");
        } else {
            self.remote = Some(src);
        }
        let received = Packet::read(&rx_buf[0..amt]).unwrap();
        assert_eq!(received, Packet::ACK(self.block_num));
        self.block_num = self.block_num.wrapping_add(1);

        // Read and send data packet
        let mut data = Vec::with_capacity(512);
        let res = self.file.read_512(&mut data);
        if res.expect("error reading from file") == 0 {
            return None;
        }
        let data_packet = Packet::DATA {
            block_num: self.block_num,
            data,
        };

        self.socket
            .send_to(data_packet.to_bytes().unwrap().to_slice(), &src)
            .expect(&format!(
                "cannot send packet {:?} to {:?}",
                data_packet,
                src
            ));
        Some(())
    }
}

fn wrq_whole_file_test(server_addr: &SocketAddr) -> Result<()> {
    // remore file if it was left over after a test that panicked
    let _ = fs::remove_file("./hello.txt");

    let mut scratch_buf = [0; MAX_PACKET_SIZE];

    let mut tx = WritingTransfer::start("./files/hello.txt", server_addr, "hello.txt");
    while let Some(_) = tx.step(&mut scratch_buf) {}

    // Would cause server to have an error if not handled robustly
    tx.socket.send_to(&[1, 2, 3], &tx.remote.unwrap())?;

    assert_files_identical("./hello.txt", "./files/hello.txt");
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

struct ReadingTransfer {
    socket: UdpSocket,
    file: File,
    block_num: u16,
    remote: Option<SocketAddr>,
}

impl ReadingTransfer {
    fn start(local_file: &str, server_addr: &SocketAddr, server_file: &str) -> Self {
        let xfer = Self {
            socket: create_socket(Some(Duration::from_secs(TIMEOUT))).unwrap(),
            file: File::create(local_file).expect(&format!("cannot create {}", local_file)),
            block_num: 1,
            remote: None,
        };
        let init_packet = Packet::RRQ {
            filename: server_file.into(),
            mode: "octet".into(),
        };
        xfer.socket
            .send_to(init_packet.to_bytes().unwrap().to_slice(), &server_addr)
            .expect(&format!(
                "cannot send initial packet {:?} to {:?}",
                init_packet,
                server_addr
            ));
        xfer
    }

    fn step(&mut self, rx_buf: &mut [u8; MAX_PACKET_SIZE]) -> Option<()> {
        let (amt, src) = self.socket.recv_from(rx_buf).expect("cannot receive");
        if self.remote.is_some() {
            assert_eq!(self.remote.unwrap(), src, "transfer source changed");
        } else {
            self.remote = Some(src);
        }

        let received = Packet::read(&rx_buf[0..amt]).unwrap();
        if let Packet::DATA { block_num, data } = received {
            assert_eq!(self.block_num, block_num);
            self.file.write_all(&data).expect(
                "cannot write to local file",
            );

            let ack_packet = Packet::ACK(self.block_num);
            self.socket
                .send_to(ack_packet.to_bytes().unwrap().to_slice(), &src)
                .expect(&format!("cannot send packet {:?} to {:?}", ack_packet, src));

            self.block_num = self.block_num.wrapping_add(1);

            if data.len() < 512 {
                return None;
            }
        } else {
            panic!("Reply packet is not a data packet");
        }
        Some(())
    }
}

fn rrq_whole_file_test(server_addr: &SocketAddr) -> Result<()> {
    let mut scratch_buf = [0; MAX_PACKET_SIZE];

    let mut rx = ReadingTransfer::start("./hello.txt", server_addr, "./files/hello.txt");
    while let Some(_) = rx.step(&mut scratch_buf) {}

    // Would cause server to have an error if not handled robustly
    rx.socket.send_to(&[1, 2, 3], &rx.remote.unwrap())?;

    assert_files_identical("./hello.txt", "./files/hello.txt");
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

fn wrq_file_exists_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(None)?;
    let init_packet = Packet::WRQ {
        filename: "./files/hello.txt".into(),
        mode: "octet".into(),
    };
    socket.send_to(
        init_packet.into_bytes()?.to_slice(),
        server_addr,
    )?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let packet = Packet::read(&buf[0..amt])?;
    if let Packet::ERROR { code, .. } = packet {
        assert_eq!(code, ErrorCode::FileExists);
    } else {
        panic!(format!("Packet has to be error packet, got: {:?}", packet));
    }
    Ok(())
}

fn rrq_file_not_found_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(None)?;
    let init_packet = Packet::RRQ {
        filename: "./hello.txt".into(),
        mode: "octet".into(),
    };
    socket.send_to(
        init_packet.into_bytes()?.to_slice(),
        server_addr,
    )?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let packet = Packet::read(&buf[0..amt])?;
    if let Packet::ERROR { code, .. } = packet {
        assert_eq!(code, ErrorCode::FileNotFound);
    } else {
        panic!(format!("Packet has to be error packet, got: {:?}", packet));
    }
    Ok(())
}

fn interleaved_read_read_same_file(server_addr: &SocketAddr) {
    let mut scratch_buf = [0; MAX_PACKET_SIZE];

    let mut read_a = ReadingTransfer::start("./read_a.txt", server_addr, "./files/hello.txt");
    let mut read_b = ReadingTransfer::start("./read_b.txt", server_addr, "./files/hello.txt");
    loop {
        let res_a = read_a.step(&mut scratch_buf);
        let res_b = read_b.step(&mut scratch_buf);
        assert_eq!(res_a, res_b, "reads finished in different number of steps");
        if res_a == None {
            break;
        }
    }

    assert_files_identical("./read_a.txt", "./files/hello.txt");
    assert_files_identical("./read_a.txt", "./read_b.txt");
    assert!(fs::remove_file("./read_a.txt").is_ok());
    assert!(fs::remove_file("./read_b.txt").is_ok());
}

fn main() {
    env_logger::init().unwrap();
    let addrs = start_server().unwrap();
    for addr in &addrs {
        wrq_whole_file_test(addr).unwrap();
        rrq_whole_file_test(addr).unwrap();
    }
    let server_addr = addrs[0];
    timeout_test(&server_addr).unwrap();
    wrq_file_exists_test(&server_addr).unwrap();
    rrq_file_not_found_test(&server_addr).unwrap();
    interleaved_read_read_same_file(&server_addr);
}
