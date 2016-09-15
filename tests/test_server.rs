extern crate tftp_server;

use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use tftp_server::packet::{Packet, MAX_PACKET_SIZE};
use tftp_server::server::TftpServer;

pub const SERVER_PORT: i32 = 3554;
pub const CLIENT_PORT: i32 = 3553;

fn port_addr(port: i32) -> SocketAddr {
    SocketAddr::from_str(format!("127.0.0.1:{}", port).as_str())
        .expect("Error creating address from port")
}

/// Starts the server in a new thread.
pub fn start_server(addr: SocketAddr) {
    let mut server_thread = thread::spawn(move || {
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
    let mut socket = UdpSocket::bind(client_addr).expect("Error creating client socket");
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

fn sample_test(server_addr: &SocketAddr) {
    let client_addr = port_addr(CLIENT_PORT);
    let input_packets = vec![Packet::WRQ {
                                 filename: "hello.txt".to_string(),
                                 mode: "octet".to_string(),
                             }];
    let expected_packets = vec![Packet::ACK(0)];
    test_tftp(&client_addr, server_addr, input_packets, expected_packets);
}

fn main() {
    let server_addr = port_addr(SERVER_PORT);
    start_server(server_addr.clone());
    sample_test(&server_addr);
}
