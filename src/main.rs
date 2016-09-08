extern crate mio;

mod data;
mod server;

use server::TFTPServer;
use std::env;
use std::str::FromStr;
use std::net::SocketAddr;

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() > 1 {
        let addr = args[1].clone();
        let socket_addr = SocketAddr::from_str(addr.as_str()).expect("Error parsing address");
        let mut server = TFTPServer::new(&socket_addr).expect("Error creating server");
        match server.run() {
            Ok(_) => println!("Server completed successfully!"),
            Err(e) => println!("Error: {:?}", e),
        }
    } else {
        println!("You need to provide the address for the UDP socket");
    }
}
