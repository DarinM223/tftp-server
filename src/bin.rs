extern crate env_logger;
extern crate tftp_server;

#[macro_use]
extern crate clap;

use tftp_server::server::{TftpServer, ServerConfig};
use std::str::FromStr;
use std::net::*;
use std::path::Path;

use clap::{Arg, App};

fn main() {
    env_logger::init().unwrap();

    let arg_ip = "IP address";
    let arg_dir = "Directory";

    // TODO: test argument handling
    let matches = App::new("TFTP Server")
        .version(crate_version!())
        .arg(
            Arg::with_name(arg_ip)
                .short("a")
                .long("address")
                .help("specifies the address[:port] to listen on")
                .takes_value(true)
                .value_name("IPAddr[:PORT]"),
        )
        .arg(
            Arg::with_name(arg_dir)
                .short("d")
                .long("drectory")
                .help("specifies the directory to serve (current by default)")
                .takes_value(true)
                .value_name("DIRECTORY"),
        )
        .arg(
            Arg::with_name("readonly")
                .short("r")
                .long("readonly")
                .help("rejects all write requests"),
        )
        .get_matches();

    let addr = match matches.value_of(arg_ip) {
        None => (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), None),
        Some(s) => {
            // try parsing in order: first ip:port, then just ip
            if let Ok(sk) = SocketAddr::from_str(s) {
                (sk.ip(), Some(sk.port()))
            } else if let Ok(ip) = IpAddr::from_str(s) {
                (ip, None)
            } else {
                panic!("error parsing argument \"{}\" as ip address", s);
            }
        }
    };

    let cfg = ServerConfig {
        readonly: matches.is_present("readonly"),
        addr,
        dir: match matches.value_of(arg_dir) {
            Some(dir) => {
                assert!(
                    Path::new(dir).exists(),
                    "specified path {} does not exist",
                    dir
                );
                Some(dir.into())
            }
            _ => None,
        },
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
