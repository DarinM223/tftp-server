extern crate tftp_server;

use std::fs;
use std::io::Read;
use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use tftp_server::packet::{DataBytes, Packet, MAX_PACKET_SIZE};
use tftp_server::server::TftpServer;

// TODO(DarinM223): change this to work on multiple computers with different
// default open ports.
pub const SERVER_PORT: i32 = 3554;
pub const CLIENT_PORT: i32 = 3553;

fn port_addr(port: i32) -> SocketAddr {
    SocketAddr::from_str(format!("127.0.0.1:{}", port).as_str())
        .expect("Error creating address from port")
}

/// Starts the server in a new thread.
pub fn start_server(addr: SocketAddr) {
    thread::spawn(move || {
        let mut server = TftpServer::new(&addr).expect("Error creating test server");
        if let Err(e) = server.run() {
            println!("Error with server: {:?}", e);
        }
        ()
    });
}

/// Tests the server by sending a bunch of input messages and asserting
/// that the responses are the same as in the expected.
pub fn test_tftp(client_addr: &SocketAddr,
                 server_addr: &SocketAddr,
                 input_msgs: Vec<Packet>,
                 output_msgs: Vec<Packet>) {
    let socket = UdpSocket::bind(client_addr).expect("Error creating client socket");
    socket.set_write_timeout(Some(Duration::from_secs(5)));
    socket.set_read_timeout(Some(Duration::from_secs(3)));

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
    let client_addr = port_addr(CLIENT_PORT);
    let input_packets = vec![Packet::WRQ {
                                 filename: "hello.txt".to_string(),
                                 mode: "octet".to_string(),
                             }];
    let expected_packets = vec![Packet::ACK(0)];
    test_tftp(&client_addr, server_addr, input_packets, expected_packets);

    // Test that hello.txt was created and remove hello.txt
    assert!(fs::metadata("./hello.txt").is_ok());
    assert!(fs::remove_file("./hello.txt").is_ok());
}

fn rrq_initial_data_test(server_addr: &SocketAddr) {
    let client_addr = port_addr(CLIENT_PORT);
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
    test_tftp(&client_addr, server_addr, input_packets, expected_packets);
}

fn main() {
    let server_addr = port_addr(SERVER_PORT);
    start_server(server_addr.clone());
    thread::sleep_ms(1000);
    wrq_initial_ack_test(&server_addr);
    rrq_initial_data_test(&server_addr);
}
