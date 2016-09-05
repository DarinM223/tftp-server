mod data;
mod server;

use server::TFTPServer;
use std::env;

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() > 1 {
        let addr = args[1].clone();
        let server = TFTPServer::new(&addr[..]).expect("Error creating server");
        match server.run() {
            Ok(_) => println!("Server completed successfully!"),
            Err(e) => println!("Error: {:?}", e),
        }
    } else {
        println!("You need to provide the address for the UDP socket");
    }
}
