use std::io;
use std::net::{ToSocketAddrs, UdpSocket};

pub struct TFTPServer {
    socket: UdpSocket,
}

impl TFTPServer {
    pub fn new<A: ToSocketAddrs>(addr: A) -> io::Result<TFTPServer> {
        let socket = try!(UdpSocket::bind(addr));

        Ok(TFTPServer { socket: socket })
    }

    pub fn run(&self) {
        unimplemented!()
    }
}
