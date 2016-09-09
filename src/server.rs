use data::{MAX_PACKET_SIZE, DataBytes, Packet};
use mio::*;
use mio::timer::{Timer, Timeout};
use mio::udp::UdpSocket;

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io;
use std::io::Read;
use std::net::SocketAddr;
use std::str::FromStr;

const SERVER: Token = Token(0);
const TIMER: Token = Token(1);

struct ConnectionState {
    conn: UdpSocket,
    file: File,
    timeout: Timeout,
    block_num: u16,
}

pub struct TftpServer {
    new_token: usize,
    poll: Poll,
    timer: Timer<&'static str>,
    timeout_queue: VecDeque<Token>,
    socket: UdpSocket,
    connections: HashMap<Token, ConnectionState>,
}

impl TftpServer {
    pub fn new(addr: &SocketAddr) -> io::Result<TftpServer> {
        let poll = try!(Poll::new());
        let socket = try!(UdpSocket::bind(addr));
        let timer = Timer::default();
        try!(poll.register(&socket, SERVER, Ready::readable(), PollOpt::edge()));
        try!(poll.register(&timer, TIMER, Ready::readable(), PollOpt::edge()));

        Ok(TftpServer {
            new_token: 2,
            poll: poll,
            timer: timer,
            timeout_queue: VecDeque::new(),
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

    fn register_timeout(&mut self) -> io::Result<Timeout> {
        unimplemented!()
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

        let mut file: File;
        let block_num: u16;
        let timeout: Timeout;
        // Handle the RRQ or WRQ packet
        match packet {
            Packet::RRQ { filename, mode } => {
                file = try!(File::open(filename));
                block_num = 1;

                let mut buf = [0; 512];
                try!(file.read(&mut buf));

                // Reply with first data packet with a block number of 1
                let data_packet = Packet::DATA {
                    block_num: block_num,
                    data: DataBytes(buf),
                };
                let data_bytes = try!(data_packet.bytes());
                socket.send_to(&data_bytes[..], &src);
                timeout = try!(self.register_timeout()); // TODO(DarinM223): fix this
            }
            Packet::WRQ { filename, mode } => {
                file = try!(File::create(filename));
                block_num = 0;

                // Reply with ACK with a block number of 0
                let ack_packet = Packet::ACK(block_num);
                let ack_bytes = try!(ack_packet.bytes());
                socket.send_to(&ack_bytes[..], &src);
                timeout = try!(self.register_timeout()); // TODO(DarinM223): fix this
            }
            _ => unreachable!(),
        }

        self.connections.insert(token,
                                ConnectionState {
                                    conn: socket,
                                    file: file,
                                    timeout: timeout,
                                    block_num: block_num,
                                });

        Ok(false)
    }

    fn handle_timer(&mut self) -> io::Result<bool> {
        // TODO(DarinM223): dequeue token from queue
        // TODO(DarinM223): resend last data packet
        unimplemented!()
    }

    fn handle_connection_packet(&mut self, token: Token) -> io::Result<bool> {
        if let Some(ref mut conn_state) = self.connections.get_mut(&token) {
            let mut buf = [0; MAX_PACKET_SIZE];
            let (_, src) = try!(conn_state.conn.recv_from(&mut buf)).unwrap();
            let packet = try!(Packet::read(buf));

            match packet {
                Packet::ACK(block_num) => {
                    // TODO(DarinM223): check if block num equals the connection's block num
                    // TODO(DarinM223): if true, send next packet and reset timeout
                }
                Packet::DATA { .. } => {
                    // TODO(DarinM223): write data to file
                    // TODO(DarinM223): send ACK packet
                }
                Packet::ERROR { .. } => {
                    // TODO(DarinM223): terminate connection
                }
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
                    TIMER => try!(self.handle_timer()),
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
