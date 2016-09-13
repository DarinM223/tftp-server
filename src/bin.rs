extern crate tftp_server;

use tftp_server::server::TftpServer;
use std::env;
use std::str::FromStr;
use std::net::SocketAddr;

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() > 1 {
        let port = args[1].clone();
        let addr = format!("127.0.0.1:{}", port);
        let socket_addr = SocketAddr::from_str(addr.as_str()).expect("Error parsing address");
        let mut server = TftpServer::new(&socket_addr).expect("Error creating server");
        match server.run() {
            Ok(_) => println!("Server completed successfully!"),
            Err(e) => println!("Error: {:?}", e),
        }
    } else {
        println!("You need to provide the address for the UDP socket");
    }
}
