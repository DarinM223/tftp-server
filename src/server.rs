use data::{MAX_PACKET_SIZE, Packet};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

pub struct TFTPServer {
    socket: UdpSocket,
}

impl TFTPServer {
    pub fn new<A: ToSocketAddrs>(addr: A) -> io::Result<TFTPServer> {
        let socket = try!(UdpSocket::bind(addr));

        Ok(TFTPServer { socket: socket })
    }

    pub fn handle_packet(&self,
                         addr: &SocketAddr,
                         bytes: [u8; MAX_PACKET_SIZE])
                         -> io::Result<bool> {
        let packet = try!(Packet::read(bytes));
        match packet {
            Packet::RRQ { .. } => {}
            Packet::WRQ { .. } => {}
            Packet::DATA { .. } => {}
            Packet::ACK(_) => {}
            Packet::ERROR { .. } => {}
        }

        Ok(false)
    }

    pub fn run(&self) -> io::Result<()> {
        loop {
            let mut buf = [0; MAX_PACKET_SIZE];
            let (_, src) = try!(self.socket.recv_from(&mut buf));
            if try!(self.handle_packet(&src, buf)) {
                break;
            }
        }

        Ok(())
    }
}
