extern crate env_logger;
extern crate tftp_server;

use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::thread;
use std::time::Duration;
use tftp_server::packet::{ErrorCode, Packet, PacketData, MAX_PACKET_SIZE};
use tftp_server::server::{create_socket, incr_block_num, Result, TftpServer};
use tftp_server::server::Read512;

const TIMEOUT: u64 = 3;

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

pub fn check_similar_files(file1: &mut File, file2: &mut File) -> Result<()> {
    let mut buf1 = String::new();
    let mut buf2 = String::new();

    file1.read_to_string(&mut buf1)?;
    file2.read_to_string(&mut buf2)?;

    assert_eq!(buf1, buf2);
    Ok(())
}

fn timeout_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(None)?;
    let init_packet = Packet::WRQ {
        filename: "hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    socket.send_to(
        init_packet.to_bytes()?.to_slice(),
        server_addr,
    )?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let reply_packet = Packet::read(PacketData::new(buf, amt))?;
    assert_eq!(reply_packet, Packet::ACK(0));


    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let reply_packet = Packet::read(PacketData::new(buf, amt))?;
    assert_eq!(reply_packet, Packet::ACK(0));

    assert!(fs::metadata("./hello.txt").is_ok());
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

fn wrq_initial_ack_test(server_addr: &SocketAddr) -> Result<()> {
    let input = Packet::WRQ {
        filename: "hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    let expected = Packet::ACK(0);

    let socket = create_socket(Some(Duration::from_secs(TIMEOUT)))?;
    socket.send_to(input.to_bytes()?.to_slice(), server_addr)?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    assert_eq!(Packet::read(PacketData::new(buf, amt))?, expected);

    // Test that hello.txt was created and remove hello.txt
    assert!(fs::metadata("./hello.txt").is_ok());
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

fn rrq_initial_data_test(server_addr: &SocketAddr) -> Result<()> {
    let input = Packet::RRQ {
        filename: "./files/hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    let mut file = File::open("./files/hello.txt")?;
    let mut buf = Vec::with_capacity(512);
    file.read_512(&mut buf)?;
    let expected = Packet::DATA {
        block_num: 1,
        data: buf,
    };

    let socket = create_socket(Some(Duration::from_secs(TIMEOUT)))?;
    socket.send_to(input.to_bytes()?.to_slice(), server_addr)?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    assert_eq!(Packet::read(PacketData::new(buf, amt))?, expected);
    Ok(())
}

fn wrq_whole_file_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(Some(Duration::from_secs(TIMEOUT)))?;
    let init_packet = Packet::WRQ {
        filename: "hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    socket.send_to(
        init_packet.to_bytes()?.to_slice(),
        server_addr,
    )?;

    {
        let mut file = File::open("./files/hello.txt")?;
        let mut block_num = 0;
        let mut recv_src;
        loop {
            let mut reply_buf = [0; MAX_PACKET_SIZE];
            let (amt, src) = socket.recv_from(&mut reply_buf)?;
            recv_src = src;
            let reply_packet = Packet::read(PacketData::new(reply_buf, amt))?;

            assert_eq!(reply_packet, Packet::ACK(block_num));
            incr_block_num(&mut block_num);

            // Read and send data packet
            let mut buf = Vec::with_capacity(512);
            match file.read_512(&mut buf) {
                Err(_) => break,
                Ok(i) if i == 0 => break,
                Ok(i) => i,
            };
            let data_packet = Packet::DATA {
                block_num: block_num,
                data: buf,
            };
            socket.send_to(data_packet.to_bytes()?.to_slice(), &src)?;
        }

        // Would cause server to have an error if this is received.
        // Used to test if connection is closed.
        socket.send_to(&[1, 2, 3], &recv_src)?;
    }

    assert!(fs::metadata("./hello.txt").is_ok());
    let (mut f1, mut f2) = (File::open("./hello.txt")?, File::open("./files/hello.txt")?);
    check_similar_files(&mut f1, &mut f2)?;
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

fn rrq_whole_file_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(Some(Duration::from_secs(TIMEOUT)))?;
    let init_packet = Packet::RRQ {
        filename: "./files/hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    socket.send_to(
        init_packet.to_bytes()?.to_slice(),
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
            let reply_packet = Packet::read(PacketData::new(reply_buf, amt))?;
            if let Packet::DATA { block_num, data } = reply_packet {
                assert_eq!(client_block_num, block_num);
                file.write_all(&data)?;

                let ack_packet = Packet::ACK(client_block_num);
                socket.send_to(ack_packet.to_bytes()?.to_slice(), &src)?;

                incr_block_num(&mut client_block_num);

                if data.len() < 512 {
                    break;
                }
            } else {
                panic!("Reply packet is not a data packet");
            }
        }

        // Would cause server to have an error if this is received.
        // Used to test if connection is closed.
        socket.send_to(&[1, 2, 3], &recv_src)?;
    }

    assert!(fs::metadata("./hello.txt").is_ok());
    let (mut f1, mut f2) = (File::open("./hello.txt")?, File::open("./files/hello.txt")?);
    check_similar_files(&mut f1, &mut f2)?;
    assert!(fs::remove_file("./hello.txt").is_ok());
    Ok(())
}

fn wrq_file_exists_test(server_addr: &SocketAddr) -> Result<()> {
    let socket = create_socket(None)?;
    let init_packet = Packet::WRQ {
        filename: "./files/hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    socket.send_to(
        init_packet.to_bytes()?.to_slice(),
        server_addr,
    )?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let packet = Packet::read(PacketData::new(buf, amt))?;
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
        filename: "./hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    socket.send_to(
        init_packet.to_bytes()?.to_slice(),
        server_addr,
    )?;

    let mut buf = [0; MAX_PACKET_SIZE];
    let amt = socket.recv(&mut buf)?;
    let packet = Packet::read(PacketData::new(buf, amt))?;
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
    thread::sleep(Duration::from_millis(1000));
    wrq_initial_ack_test(&server_addr).unwrap();
    rrq_initial_data_test(&server_addr).unwrap();
    thread::sleep(Duration::from_millis(1000));
    wrq_whole_file_test(&server_addr).unwrap();
    rrq_whole_file_test(&server_addr).unwrap();
    timeout_test(&server_addr).unwrap();
    wrq_file_exists_test(&server_addr).unwrap();
    rrq_file_not_found_test(&server_addr).unwrap();
}
