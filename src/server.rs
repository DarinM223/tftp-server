use data::{MAX_PACKET_SIZE, Packet};
use mio::*;
use mio::udp::UdpSocket;

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::str::FromStr;

const SERVER: Token = Token(0);

struct ConnectionState {
    conn: UdpSocket,
}

pub struct TftpServer {
    new_token: usize,
    poll: Poll,
    socket: UdpSocket,
    connections: HashMap<Token, ConnectionState>,
}

impl TftpServer {
    pub fn new(addr: &SocketAddr) -> io::Result<TftpServer> {
        let poll = try!(Poll::new());
        let socket = try!(UdpSocket::bind(addr));
        try!(poll.register(&socket, SERVER, Ready::readable(), PollOpt::edge()));

        Ok(TftpServer {
            new_token: 1,
            poll: poll,
            socket: socket,
            connections: HashMap::new(),
        })
    }

    fn generate_port_number(&self) -> i32 {
        unimplemented!()
    }

    fn generate_token(&mut self) -> Token {
        let token = Token(self.new_token);
        self.new_token += 1;
        token
    }

    fn handle_server_packet(&mut self) -> io::Result<bool> {
        let mut buf = [0; MAX_PACKET_SIZE];
        let (_, src) = try!(self.socket.recv_from(&mut buf)).unwrap();
        let packet = try!(Packet::read(buf));
        // Only allow RRQ and WRQ packets to be received
        match packet {
            Packet::RRQ { .. } |
            Packet::WRQ { .. } => {}
            _ => {
                println!("Error: Received invalid packet");
                return Ok(false);
            }
        }

        let port_number = self.generate_port_number();
        let addr = format!("127.0.0.1:{}", port_number);
        let socket_addr = SocketAddr::from_str(addr.as_str()).unwrap();
        let socket: UdpSocket = try!(UdpSocket::bind(&socket_addr));
        let token = self.generate_token();

        try!(self.poll.register(&socket, token, Ready::readable(), PollOpt::edge()));

        // Handle the RRQ or WRQ packet
        match packet {
            Packet::RRQ { filename, mode } => {
                // TODO(DarinM223): Open file for reading
                // TODO(DarinM223): Reply with first data packet
            }
            Packet::WRQ { filename, mode } => {
                // TODO(DarinM223): Open file for writing
                // TODO(DarinM223): Reply with ACK with a block number of 0
            }
            _ => {}
        }

        self.connections.insert(token, ConnectionState { conn: socket });

        Ok(false)
    }

    fn handle_connection_packet(&mut self, token: Token) -> io::Result<bool> {
        if let Some(ref mut conn_state) = self.connections.get_mut(&token) {
            let mut buf = [0; MAX_PACKET_SIZE];
            let (_, src) = try!(conn_state.conn.recv_from(&mut buf)).unwrap();
            let packet = try!(Packet::read(buf));

            match packet {
                Packet::ACK(_) => {}
                Packet::DATA { .. } => {}
                Packet::ERROR { .. } => {}
                _ => {}
            }
        }

        Ok(false)
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut events = Events::with_capacity(1024);
        'main_loop: loop {
            try!(self.poll.poll(&mut events, None));

            for event in events.iter() {
                let finished = match event.token() {
                    SERVER => try!(self.handle_server_packet()),
                    token if self.connections.get(&token).is_some() => {
                        try!(self.handle_connection_packet(token))
                    }
                    _ => unreachable!(),
                };
                if finished {
                    break 'main_loop;
                }
            }
        }

        Ok(())
    }
}
