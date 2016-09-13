extern crate tftp_server;

use std::net::{SocketAddr, UdpSocket};
use std::str::FromStr;
use std::thread;
use tftp_server::packet::{Packet, MAX_PACKET_SIZE};
use tftp_server::server::TftpServer;

pub const SERVER_PORT: i32 = 123;
pub const CLIENT_PORT: i32 = 456;

fn port_addr(port: i32) -> SocketAddr {
    SocketAddr::from_str(format!("127.0.0.1:{}", port).as_str())
        .expect("Error creating address from port")
}

/// Spawns the server and a client socket in two separate threads.
/// The client socket sends the given input packets and tests the
/// response packets with the expected responses.
pub fn test_tftp(addr: &SocketAddr,
                 input_msgs: Vec<Packet>,
                 output_msgs: Vec<Packet>)
                 -> thread::Result<()> {
    let addr = addr.clone();
    let mut server_thread = thread::spawn(move || {
        let mut server = TftpServer::new(&addr).expect("Error creating test server");
        if let Err(e) = server.run() {
            println!("Error with server: {:?}", e);
        }
        ()
    });

    let addr = addr.clone();
    let mut client_thread = thread::spawn(move || {
        let mut socket = UdpSocket::bind(port_addr(CLIENT_PORT))
            .expect("Error creating client socket");

        for (input, output) in input_msgs.into_iter().zip(output_msgs.into_iter()) {
            let input_bytes = input.bytes().expect("Error creating input packet");
            socket.send_to(&input_bytes[..], &addr).expect("Error sending message");

            let mut reply_buf = [0; MAX_PACKET_SIZE];
            socket.recv_from(&mut reply_buf).expect("Error receiving reply from socket");
            let reply_packet = Packet::read(reply_buf).expect("Error creating output packet");
            assert_eq!(reply_packet, output);
        }
        ()
    });

    try!(server_thread.join());
    try!(client_thread.join());
    Ok(())
}

#[test]
fn sample_test() {
    // TODO(DarinM223): create a list of input packets to send to the server
    // and a list of output packets that the server should reply back
    // and then call test_tftp with the two lists.
}
