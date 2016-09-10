use data::{MAX_PACKET_SIZE, DataBytes, Packet};
use mio::*;
use mio::timer::{Timer, Timeout};
use mio::udp::UdpSocket;

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

const TIMEOUT_LENGTH: u64 = 5;
const SERVER: Token = Token(0);
const TIMER: Token = Token(1);

struct ConnectionState {
    conn: UdpSocket,
    file: File,
    timeout: Timeout,
    block_num: u16,
    last_packet: Packet,
    addr: SocketAddr,
}

pub struct TftpServer {
    new_token: usize,
    poll: Poll,
    timer: Timer<Token>,
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

        let mut file: File;
        let block_num: u16;
        let timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT_LENGTH), token).unwrap();
        let last_packet: Packet;
        // Handle the RRQ or WRQ packet
        match packet {
            Packet::RRQ { filename, mode } => {
                file = try!(File::open(filename));
                block_num = 1;

                let mut buf = [0; 512];
                try!(file.read(&mut buf));

                // Reply with first data packet with a block number of 1
                last_packet = Packet::DATA {
                    block_num: block_num,
                    data: DataBytes(buf),
                };
            }
            Packet::WRQ { filename, mode } => {
                file = try!(File::create(filename));
                block_num = 0;

                // Reply with ACK with a block number of 0
                last_packet = Packet::ACK(block_num);
            }
            _ => unreachable!(),
        }

        let packet_bytes = try!(last_packet.clone().bytes());
        try!(socket.send_to(&packet_bytes[..], &src));

        self.connections.insert(token,
                                ConnectionState {
                                    conn: socket,
                                    file: file,
                                    timeout: timeout,
                                    block_num: block_num,
                                    last_packet: last_packet,
                                    addr: src,
                                });

        Ok(false)
    }

    fn handle_timer(&mut self) -> io::Result<bool> {
        let token = self.timer.poll().unwrap();
        if let Some(ref mut conn) = self.connections.get_mut(&token) {
            let last_packet = conn.last_packet.clone();
            let last_packet_bytes = try!(last_packet.bytes());
            try!(conn.conn.send_to(&last_packet_bytes[..], &conn.addr));
        }

        Ok(false)
    }

    fn handle_connection_packet(&mut self, token: Token) -> io::Result<bool> {
        if let Some(ref mut conn) = self.connections.get_mut(&token) {
            let mut buf = [0; MAX_PACKET_SIZE];
            let (_, src) = try!(conn.conn.recv_from(&mut buf)).unwrap();
            let packet = try!(Packet::read(buf));

            match packet {
                Packet::ACK(block_num) => {
                    if block_num != conn.block_num {
                        // TODO(DarinM223): handle error
                        panic!("Invalid block number received");
                    }

                    conn.block_num += 1;
                    let mut buf = [0; 512];
                    try!(conn.file.read(&mut buf));

                    // Send next data packet
                    let packet = Packet::DATA {
                        block_num: conn.block_num,
                        data: DataBytes(buf),
                    };
                    let packet_bytes = try!(packet.bytes());
                    try!(conn.conn.send_to(&packet_bytes[..], &conn.addr));
                }
                Packet::DATA { block_num, data } => {
                    if block_num != conn.block_num + 1 {
                        // TODO(DarinM223): handle error
                        panic!("Invalid block number received");
                    }

                    conn.block_num += 1;
                    try!(conn.file.write(&data.0[..]));

                    // Send ACK packet for data
                    let packet = Packet::ACK(conn.block_num);
                    let packet_bytes = try!(packet.bytes());
                    try!(conn.conn.send_to(&packet_bytes[..], &conn.addr));
                }
                Packet::ERROR { .. } => {
                    // TODO(DarinM223): terminate connection
                    panic!("Error!");
                }
                _ => {}
            }

            // Reset timeout
            self.timer.cancel_timeout(&conn.timeout);
            self.timer.set_timeout(Duration::from_secs(TIMEOUT_LENGTH), token);
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
