extern crate tftp_server;

use std::fs;
use std::io::Read;
use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use tftp_server::packet::{DataBytes, Packet, MAX_PACKET_SIZE};
use tftp_server::server::{create_socket, TftpServer};

const TIMEOUT: u64 = 3;

/// Starts the server in a new thread.
pub fn start_server() -> SocketAddr {
    let mut server = TftpServer::new().expect("Error creating test server");
    let addr = server.local_addr().expect("Error getting address from server").clone();
    thread::spawn(move || {
        if let Err(e) = server.run() {
            println!("Error with server: {:?}", e);
        }
        ()
    });
    addr
}

pub fn get_socket(addr: &SocketAddr) -> UdpSocket {
    let socket = UdpSocket::bind(addr).expect("Error creating client socket");
    socket.set_write_timeout(Some(Duration::from_secs(5)));
    socket.set_read_timeout(Some(Duration::from_secs(3)));
    socket
}

/// Tests the server by sending a bunch of input messages and asserting
/// that the responses are the same as in the expected.
pub fn test_tftp(server_addr: &SocketAddr, input_msgs: Vec<Packet>, output_msgs: Vec<Packet>) {
    let socket = create_socket(Duration::from_secs(TIMEOUT)).expect("Error creating client socket");
    for (input, output) in input_msgs.into_iter().zip(output_msgs.into_iter()) {
        let input_bytes = input.bytes().expect("Error creating input packet");
        socket.send_to(&input_bytes[..], server_addr).expect("Error sending message");

        let mut reply_buf = [0; MAX_PACKET_SIZE];
        socket.recv_from(&mut reply_buf).expect("Error receiving reply from socket");
        let reply_packet = Packet::read(reply_buf).expect("Error creating output packet");
        assert_eq!(reply_packet, output);
    }
}

fn wrq_initial_ack_test(server_addr: &SocketAddr) {
    let input_packets = vec![Packet::WRQ {
                                 filename: "hello.txt".to_string(),
                                 mode: "octet".to_string(),
                             }];
    let expected_packets = vec![Packet::ACK(0)];
    test_tftp(server_addr, input_packets, expected_packets);

    // Test that hello.txt was created and remove hello.txt
    assert!(fs::metadata("./hello.txt").is_ok());
    assert!(fs::remove_file("./hello.txt").is_ok());
}

fn rrq_initial_data_test(server_addr: &SocketAddr) {
    let input_packets = vec![Packet::RRQ {
                                 filename: "./files/hello.txt".to_string(),
                                 mode: "octet".to_string(),
                             }];
    let mut file = fs::File::open("./files/hello.txt").expect("Error opening test file");
    let mut buf = [0; 512];
    file.read(&mut buf).expect("Error reading from test file");
    let expected_packets = vec![Packet::DATA {
                                    block_num: 1,
                                    data: DataBytes(buf),
                                }];
    test_tftp(server_addr, input_packets, expected_packets);
}

fn wrq_whole_file_test(server_addr: &SocketAddr) {
    let socket = create_socket(Duration::from_secs(TIMEOUT)).expect("Error creating socket");
    let init_packet = Packet::WRQ {
        filename: "hello.txt".to_string(),
        mode: "octet".to_string(),
    };
    let init_packet_bytes = init_packet.bytes().expect("Error creating init packet");
    socket.send_to(&init_packet_bytes[..], server_addr).expect("Error sending init packet");

    let mut file = fs::File::open("./files/hello.txt").expect("Error opening test file");
    let mut block_num = 0;
    loop {
        let mut reply_buf = [0; MAX_PACKET_SIZE];
        let (_, src) = socket.recv_from(&mut reply_buf).expect("Error receiving reply from socket");
        let reply_packet = Packet::read(reply_buf).expect("Error creating reply packet");

        assert_eq!(reply_packet, Packet::ACK(block_num));
        block_num += 1;

        // Read and send data packet
        let mut buf = [0; 512];
        if let Err(_) = file.read(&mut buf) {
            break;
        }
        let data_packet = Packet::DATA {
            block_num: block_num,
            data: DataBytes(buf),
        };
        let data_packet_bytes = data_packet.bytes().expect("Error creating data packet");
        socket.send_to(&data_packet_bytes[..], &src);
    }

    assert!(fs::metadata("./hello.txt").is_ok());
    // TODO(DarinM223): test that file is identical to ./files/hello.txt
    assert!(fs::remove_file("./hello.txt").is_ok());
}

fn rrq_whole_file_test(server_addr: &SocketAddr) {
    unimplemented!()
}

fn main() {
    let server_addr = start_server();
    thread::sleep_ms(1000);
    wrq_initial_ack_test(&server_addr);
    rrq_initial_data_test(&server_addr);
    wrq_whole_file_test(&server_addr);
}
