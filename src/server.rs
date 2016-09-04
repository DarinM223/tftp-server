use data::{MAX_PACKET_SIZE, merge_bytes, OpCode, Packet};
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

    pub fn handle_packet(&self, addr: &SocketAddr, bytes: [u8; MAX_PACKET_SIZE]) -> bool {
        let packet = Packet::read(bytes);
        match packet {
            Packet::RRQ { .. } => {}
            Packet::WRQ { .. } => {}
            Packet::DATA { .. } => {}
            Packet::ACK(_) => {}
            Packet::ERROR { .. } => {}
        }
        false
    }

    pub fn run(&self) -> io::Result<()> {
        while true {
            let mut buf = [0; MAX_PACKET_SIZE];
            let (_, src) = try!(self.socket.recv_from(&mut buf));
            if self.handle_packet(&src, buf) {
                break;
            }
        }

        Ok(())
    }
}
