extern crate env_logger;
extern crate tftp_server;

#[macro_use]
extern crate clap;

use tftp_server::server::TftpServer;
use std::env;
use std::str::FromStr;
use std::net::SocketAddr;

use clap::{Arg, App};

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("My Super Program")
                          .version(crate_version!())
                          .about("A TFTP server")
                          .arg(Arg::with_name("directory")
                               .short("d")
                               .long("directory")
                               .value_name("DIR")
                               .help("serves the specified directory instead of the current one")
                               .takes_value(true))
                          .arg(Arg::with_name("readonly")
                               .short("r")
                               .long("readonly")
                               .help("rejects all write requests to the served directory"))
                          .arg(Arg::with_name("IPv4")
                               .short("4")
                               .long("ipv4")
                               .help("enables IPv4"))
                          .arg(Arg::with_name("IPv6")
                               .short("6")
                               .long("ipv6")
                               .help("enables IPv6"))
                          .get_matches();

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
