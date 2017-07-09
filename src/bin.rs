extern crate env_logger;
extern crate tftp_server;

use tftp_server::server::TftpServer;
use std::env;
use std::str::FromStr;
use std::net::SocketAddr;

fn main() {
    env_logger::init().unwrap();

    let args: Vec<_> = env::args().collect();
    let mut server: TftpServer;
    if args.len() > 1 {
        let port = args[1].clone();
        let addr = format!("127.0.0.1:{}", port);
        let socket_addr = SocketAddr::from_str(addr.as_str()).expect("Error parsing address");
        server = TftpServer::new_from_addr(&socket_addr).expect("Error creating server");
    } else {
        server = TftpServer::new().expect("Error creating server");
        println!(
            "Server created at address: {:?}",
            server.local_addr().unwrap()
        );
    }

    match server.run() {
        Ok(_) => println!("Server completed successfully!"),
        Err(e) => println!("Error: {:?}", e),
    }
}
