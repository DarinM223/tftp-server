extern crate env_logger;
extern crate tftp_server;

#[macro_use]
extern crate clap;

use tftp_server::server::{TftpServer, ServerConfig};
use std::str::FromStr;
use std::net::{Ipv4Addr, SocketAddrV4};

use clap::{Arg, App};

fn main() {
    env_logger::init().unwrap();

    let arg_ipv4 = "IPv4 address";
    let matches = App::new("TFTP Server")
        .version(crate_version!())
        .arg(
            Arg::with_name(arg_ipv4)
                .short("4")
                .long("ipv4")
                .help("specifies the ipv4 address[:port] to listen on")
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

    let v4addr = match matches.value_of(arg_ipv4) {
        None => (Ipv4Addr::new(127, 0, 0, 1), None),
        Some(s) => {
            // try parsing in order: first ipv4:port, then just ipv4
            if let Ok(sk4) = SocketAddrV4::from_str(s) {
                (*sk4.ip(), Some(sk4.port()))
            } else if let Ok(v4) = Ipv4Addr::from_str(s) {
                (v4, None)
            } else {
                panic!("error parsing argument \"{}\" as ipv4 address", s);
            }
        }
    };

    let cfg = ServerConfig {
        readonly: matches.is_present("readonly"),
        v4addr,
    };

    let mut server = TftpServer::with_cfg(&cfg).expect("Error creating server");
    println!(
        "Server created at address: {:?}",
        server.local_addr().unwrap()
    );

    match server.run() {
        Ok(_) => println!("Server completed successfully!"),
        Err(e) => println!("Error: {:?}", e),
    }
}
