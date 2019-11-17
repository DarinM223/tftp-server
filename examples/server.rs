extern crate env_logger;
extern crate getopts;
extern crate log;
extern crate tftp_server;

use getopts::Options;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use tftp_server::server::TftpServerBuilder;

fn main() {
    env_logger::init().unwrap();

    let args: Vec<_> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt("p", "port", "Sets the port the server runs on", "PORT");
    opts.optopt(
        "d",
        "directory",
        "Sets the directory the server serves files on",
        "PATH",
    );
    opts.optflag("h", "help", "Print help menu");
    let matches = opts.parse(&args[1..]).unwrap();
    if matches.opt_present("h") {
        let brief = format!("Usage: {} [options]", &program);
        print!("{}", opts.usage(&brief));
        return;
    }

    let socket_addr = matches
        .opt_str("p")
        .map(|p| format!("127.0.0.1:{}", p))
        .map(|addr| SocketAddr::from_str(addr.as_str()).expect("Error parsing address"));
    let dir = matches.opt_str("d").map(|s| PathBuf::from(s));
    let mut server = TftpServerBuilder::new()
        .addr_opt(socket_addr)
        .serve_dir_opt(dir)
        .build()
        .expect("Error creating server");
    println!(
        "Server created at address {:?}",
        server.local_addr().unwrap()
    );

    match server.run() {
        Ok(_) => println!("Server completed successfully!"),
        Err(e) => println!("Error: {:?}", e),
    }
}
