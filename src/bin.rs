extern crate env_logger;
extern crate tftp_server;

#[macro_use]
extern crate clap;

use tftp_server::server::{TftpServer, ServerConfig};
use std::str::FromStr;
use std::net::*;
use std::path::Path;
use std::time::Duration;

use clap::{Arg, App};

fn main() {
    env_logger::init().unwrap();

    let arg_ip = "IP address";
    let arg_dir = "Directory";
    let arg_timeout = "Timeout";
    let arg_readonly = "Readonly";

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
            Arg::with_name(arg_timeout)
                .short("t")
                .long("timeout")
                .help(
                    "the (natural) number of seconds before an idle transfer is terminated",
                )
                .takes_value(true)
                .value_name("SECONDS"),
        )
        .arg(
            Arg::with_name(arg_readonly)
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

    let timeout = Duration::from_secs(
        matches
            .value_of(arg_timeout)
            .map(|s| {
                u64::from_str(s).expect(&format!("error parsing {} as timeout", s))
            })
            .map(|n| if n == 0 {
                panic!("timeout may not be 0 seconds")
            } else {
                n
            })
            .unwrap_or(3),
    );

    let cfg = ServerConfig {
        readonly: matches.is_present(arg_readonly),
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
        timeout,
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
