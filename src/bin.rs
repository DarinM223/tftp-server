extern crate env_logger;
extern crate tftp_server;

#[macro_use]
extern crate clap;

use tftp_server::server::{TftpServer, ServerConfig};
use std::str::FromStr;
use std::net::SocketAddrV4;

use clap::{Arg, App};

fn main() {
    env_logger::init().unwrap();

    let matches = App::new("TFTP Server")
        .version(crate_version!())
        .arg(
            Arg::with_name("IPv4 address")
                .short("4a")
                .long("ipv4-address")
                .help("specifies the ipv4 address:port to listen on")
                .takes_value(true)
                .value_name("IPv4Addr[:PORT]"),
        )
        .arg(
            Arg::with_name("readonly")
                .short("r")
                .long("readonly")
                .help("rejects all write requests"),
        )
        .get_matches();

    let cfg = ServerConfig {
        readonly: matches.is_present("readonly"),
        v4addr: matches.value_of("ipv4-address").map(|s| {
            SocketAddrV4::from_str(s).ok().expect(
                "error parsing ipv4 address",
            )
        }),
    };

    let mut server = TftpServer::new(&cfg).expect("Error creating server");
    println!(
        "Server created at address: {:?}",
        server.local_addr().unwrap()
    );

    match server.run() {
        Ok(_) => println!("Server completed successfully!"),
        Err(e) => println!("Error: {:?}", e),
    }
}
