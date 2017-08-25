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
pub fn start_server() -> Result<SocketAddr> {
    let mut server = TftpServer::new()?;
    let addr = server.local_addr()?;
    thread::spawn(move || {
        if let Err(e) = server.run() {
            println!("Error with server: {:?}", e);
        }
        ()
    });

    Ok(addr)
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
    server_file: String,
    remote: Option<SocketAddr>,
    server_addr: SocketAddr,
    first: bool,
}

impl WritingTransfer {
    fn new(from: &str, server_addr: &SocketAddr, to: &str) -> Self {
        Self {
            socket: create_socket(Some(Duration::from_secs(TIMEOUT))).unwrap(),
            file: File::open(from).expect(&format!("cannot open {}", from)),
            block_num: 0,
            server_file: to.into(),
            remote: None,
            server_addr: *server_addr,
            first: true,
        }
    }

    fn step(&mut self, rx_buf: &mut [u8; MAX_PACKET_SIZE]) -> Option<()> {
        let (pack, dest) = if self.first {
            self.first = false;
            let init_packet = Packet::WRQ {
                filename: self.server_file.clone(),
                mode: "octet".into(),
            };
            (init_packet, self.server_addr)
        } else {
            let (amt, src) = self.socket.recv_from(rx_buf).expect("cannot receive");
            if self.remote.is_some() {
                assert_eq!(self.remote.unwrap(), src, "transfer source changed");
            } else {
                self.remote = Some(src);
            }
            let expected = Packet::read(&rx_buf[0..amt]).unwrap();
            assert_eq!(expected, Packet::ACK(self.block_num));
            self.block_num = self.block_num.wrapping_add(1);

            // Read and send data packet
            let mut data = Vec::with_capacity(512);
            match self.file.read_512(&mut data) {
                Err(_) | Ok(0) => return None,
                _ => {}
            };
            let data_packet = Packet::DATA {
                block_num: self.block_num,
                data,
            };
            (data_packet, src)
        };

        self.socket
            .send_to(pack.to_bytes().unwrap().to_slice(), &dest)
            .expect(&format!("cannot send packet {:?} to {:?}", pack, dest));
        Some(())
    }
}

fn wrq_whole_file_test(server_addr: &SocketAddr) -> Result<()> {
    // remore file if it was left over after a test that panicked
    let _ = fs::remove_file("./hello.txt");

    let mut tx = WritingTransfer::new("./files/hello.txt", server_addr, "hello.txt");

    let mut scratch_buf = [0; MAX_PACKET_SIZE];
    while let Some(_) = tx.step(&mut scratch_buf) {}

    // Would cause server to have an error if not handled robustly
    tx.socket.send_to(&[1, 2, 3], &tx.remote.unwrap())?;

    assert_files_identical("./hello.txt", "./files/hello.txt");
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

fn rrq_whole_file_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(Some(Duration::from_secs(TIMEOUT)))?;
    let init_packet = Packet::RRQ {
        filename: "./files/hello.txt".into(),
        mode: "octet".into(),
    };
    socket.send_to(
        init_packet.into_bytes()?.to_slice(),
        server_addr,
    )?;

    {
        let mut file = File::create("./hello.txt")?;
        let mut client_block_num = 1;
        let mut recv_src;
        loop {
            let mut reply_buf = [0; MAX_PACKET_SIZE];
            let (amt, src) = socket.recv_from(&mut reply_buf)?;
            recv_src = src;
            let reply_packet = Packet::read(&reply_buf[0..amt])?;
            if let Packet::DATA { block_num, data } = reply_packet {
                assert_eq!(client_block_num, block_num);
                file.write_all(&data)?;

                let ack_packet = Packet::ACK(client_block_num);
                socket.send_to(ack_packet.into_bytes()?.to_slice(), &src)?;

                client_block_num = client_block_num.wrapping_add(1);

                if data.len() < 512 {
                    break;
                }
            } else {
                panic!("Reply packet is not a data packet");
            }
        }

        // Would cause server to have an error if not handled robustly
        socket.send_to(&[1, 2, 3], &recv_src)?;
    }

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

fn main() {
    env_logger::init().unwrap();
    let server_addr = start_server().unwrap();
    wrq_whole_file_test(&server_addr).unwrap();
    rrq_whole_file_test(&server_addr).unwrap();
    timeout_test(&server_addr).unwrap();
    wrq_file_exists_test(&server_addr).unwrap();
    rrq_file_not_found_test(&server_addr).unwrap();
}
