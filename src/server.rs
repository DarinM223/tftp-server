use data::{MAX_PACKET_SIZE, Packet};
use mio::*;
use mio::udp::UdpSocket;

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;

const SERVER: Token = Token(0);

pub struct TFTPServer {
    new_token: usize,
    poll: Poll,
    states: HashMap<Token, ()>,
}

impl TFTPServer {
    pub fn new(addr: &SocketAddr) -> io::Result<TFTPServer> {
        let poll: Poll = try!(Poll::new());
        let socket: UdpSocket = try!(UdpSocket::bind(addr));
        try!(poll.register(&socket, SERVER, Ready::readable(), PollOpt::edge()));

        Ok(TFTPServer {
            new_token: 1,
            poll: poll,
            states: HashMap::new(),
        })
    }

    fn generate_port_number(&self) -> i32 {
        unimplemented!()
    }

    fn handle_server_packet(&mut self) -> io::Result<bool> {
        unimplemented!()
    }

    fn handle_connection_packet(&mut self, token: Token) -> io::Result<bool> {
        unimplemented!()
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut events = Events::with_capacity(1024);
        'main_loop: loop {
            try!(self.poll.poll(&mut events, None));

            for event in events.iter() {
                let finished = match event.token() {
                    SERVER => try!(self.handle_server_packet()),
                    tok if self.states.get(&tok).is_some() => {
                        try!(self.handle_connection_packet(tok))
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
