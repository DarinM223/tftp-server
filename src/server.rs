use mio::*;
use mio::timer::{Timer, TimerError, Timeout};
use mio::udp::UdpSocket;
use packet::{MAX_PACKET_SIZE, DataBytes, Packet, PacketData, PacketErr};
use rand;
use rand::Rng;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::net;
use std::net::SocketAddr;
use std::result;
use std::str::FromStr;
use std::time::Duration;
use std::u16;

const TIMEOUT: u64 = 3;
const SERVER: Token = Token(0);
const TIMER: Token = Token(1);

#[derive(Debug)]
pub enum TftpError {
    PacketError(PacketErr),
    IoError(io::Error),
    TimerError(TimerError),
    NoOpenSocket,
    CloseConnection,
    NoneFromSocket,
}

impl From<io::Error> for TftpError {
    fn from(err: io::Error) -> TftpError {
        TftpError::IoError(err)
    }
}

impl From<PacketErr> for TftpError {
    fn from(err: PacketErr) -> TftpError {
        TftpError::PacketError(err)
    }
}

impl From<TimerError> for TftpError {
    fn from(err: TimerError) -> TftpError {
        TftpError::TimerError(err)
    }
}

pub type Result<T> = result::Result<T, TftpError>;

/// The state contained within a connection.
/// A connection is started when a server socket receives
/// a RRQ or a WRQ packet and ends when the connection socket
/// receives a DATA packet less than 516 bytes or if the connection
/// socket receives an invalid packet.
struct ConnectionState {
    /// The UDP socket for the connection that receives ACK, DATA, or ERROR packets.
    conn: UdpSocket,
    /// The open file either being written to or read from during the transfer.
    /// If the connection was started with a RRQ, the file would be read from, if it
    /// was started with a WRQ, the file would be written to.
    file: File,
    /// The timeout for the last packet. Every time a new packet is received, the
    /// timeout is reset.
    timeout: Timeout,
    /// The current block number of the transfer. If the block numbers of the received packet
    /// and the current block number do not match, the connection is closed.
    block_num: u16,
    /// The last packet sent. This is used when a timeout happens to resend the last packet.
    last_packet: Packet,
    /// The address of the client socket to reply to.
    addr: SocketAddr,
}

pub struct TftpServer {
    /// The ID of a new token used for generating different tokens.
    new_token: usize,
    /// The event loop for handling async events.
    poll: Poll,
    /// The main timer that can be used to set multiple timeout events.
    timer: Timer<Token>,
    /// The main server socket that receives RRQ and WRQ packets
    /// and creates a new separate UDP connection.
    socket: UdpSocket,
    /// The separate UDP connections for handling multiple requests.
    connections: HashMap<Token, ConnectionState>,
}

impl TftpServer {
    /// Creates a new TFTP server from a random open UDP port.
    pub fn new() -> Result<TftpServer> {
        let poll = Poll::new()?;
        let socket = UdpSocket::from_socket(create_socket(Some(Duration::from_secs(TIMEOUT)))?)?;
        let timer = Timer::default();
        poll.register(&socket, SERVER, Ready::all(), PollOpt::edge())?;
        poll.register(&timer, TIMER, Ready::readable(), PollOpt::edge())?;

        Ok(TftpServer {
            new_token: 2,
            poll: poll,
            timer: timer,
            socket: socket,
            connections: HashMap::new(),
        })
    }

    /// Creates a new TFTP server from a socket address.
    pub fn new_from_addr(addr: &SocketAddr) -> Result<TftpServer> {
        let poll = Poll::new()?;
        let socket = UdpSocket::bind(addr)?;
        let timer = Timer::default();
        poll.register(&socket, SERVER, Ready::all(), PollOpt::edge())?;
        poll.register(&timer, TIMER, Ready::readable(), PollOpt::edge())?;

        Ok(TftpServer {
            new_token: 2,
            poll: poll,
            timer: timer,
            socket: socket,
            connections: HashMap::new(),
        })
    }


    /// Returns a new token created from incrementing a counter.
    fn generate_token(&mut self) -> Token {
        let token = Token(self.new_token);
        self.new_token += 1;
        token
    }

    fn cancel_connection(&mut self, token: &Token) -> Result<()> {
        if let Some(conn) = self.connections.remove(token) {
            self.poll.deregister(&conn.conn)?;
            self.timer.cancel_timeout(&conn.timeout).expect("Error canceling timeout");
        }
        Ok(())
    }

    fn reset_timeout(&mut self, token: &Token) -> Result<()> {
        if let Some(ref mut conn) = self.connections.get_mut(token) {
            self.timer.cancel_timeout(&conn.timeout);
            conn.timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT), *token)?;
        } else {
            panic!("No connection for token");
        }
        Ok(())
    }

    /// Handles a packet sent to the main server connection.
    /// It opens a new UDP connection in a random port and replies with either an ACK
    /// or a DATA packet depending on the whether it received an RRQ or a WRQ packet.
    fn handle_server_packet(&mut self) -> Result<()> {
        let mut buf = [0; MAX_PACKET_SIZE];
        let (amt, src) = match self.socket.recv_from(&mut buf)? {
            Some((amt, src)) => (amt, src),
            None => return Err(TftpError::NoneFromSocket),
        };
        let packet = Packet::read(PacketData::new(buf, amt))?;

        // Handle the RRQ or WRQ packet.
        let (file, block_num, send_packet) = match packet {
            Packet::RRQ { filename, mode } => handle_rrq_packet(filename, mode)?,
            Packet::WRQ { filename, mode } => handle_wrq_packet(filename, mode)?,
            _ => unreachable!(),
        };

        // Create new connection.
        let socket = UdpSocket::from_socket(create_socket(Some(Duration::from_secs(TIMEOUT)))?)?;
        let token = self.generate_token();
        let timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT), token)?;
        self.poll.register(&socket, token, Ready::all(), PollOpt::edge())?;
        println!("Created connection with token: {:?}", token);

        socket.send_to(send_packet.clone().bytes()?.to_slice(), &src)?;
        self.connections.insert(token,
                                ConnectionState {
                                    conn: socket,
                                    file: file,
                                    timeout: timeout,
                                    block_num: block_num,
                                    last_packet: send_packet,
                                    addr: src,
                                });

        Ok(())
    }

    /// Handles the event when a timer times out.
    /// It gets the connection from the token and resends
    /// the last packet sent from the connection.
    fn handle_timer(&mut self) -> Result<()> {
        let mut tokens = Vec::new();
        while let Some(token) = self.timer.poll() {
            tokens.push(token);
        }

        for token in tokens {
            if let Some(ref mut conn) = self.connections.get_mut(&token) {
                println!("Timeout: resending last packet for token: {:?}", token);
                conn.conn.send_to(conn.last_packet.clone().bytes()?.to_slice(), &conn.addr)?;
            }
            self.reset_timeout(&token);
        }

        Ok(())
    }

    /// Handles a packet sent to an open child connection.
    fn handle_connection_packet(&mut self, token: Token) -> Result<()> {
        if let Some(ref mut conn) = self.connections.get_mut(&token) {
            let mut buf = [0; MAX_PACKET_SIZE];
            let amt = match conn.conn.recv_from(&mut buf)? {
                Some((amt, _)) => amt,
                None => return Err(TftpError::NoneFromSocket),
            };
            let packet = Packet::read(PacketData::new(buf, amt))?;

            match packet {
                Packet::ACK(block_num) => handle_ack_packet(block_num, conn)?,
                Packet::DATA { block_num, data, len } => {
                    handle_data_packet(block_num, data, len, conn)?
                }
                Packet::ERROR { .. } => {
                    println!("Error message received");
                    return Err(TftpError::CloseConnection);
                }
                _ => {
                    println!("Received invalid packet from connection");
                    return Err(TftpError::CloseConnection);
                }
            }
        }

        Ok(())
    }

    /// Called for every event sent from the event loop. The event
    /// is a token that can either be from the server, from an open connection,
    /// or from a timeout timer for a connection.
    pub fn handle_token(&mut self, token: Token) -> Result<()> {
        match token {
            SERVER => {
                match self.handle_server_packet() {
                    Err(TftpError::NoneFromSocket) => {}
                    Err(e) => println!("Error: {:?}", e),
                    _ => {}
                }
            }
            token if self.connections.get(&token).is_some() => {
                match self.handle_connection_packet(token) {
                    Err(TftpError::CloseConnection) => {}
                    Err(TftpError::NoneFromSocket) => return Ok(()),
                    Err(e) => println!("Error: {:?}", e),
                    _ => {
                        self.reset_timeout(&token)?;
                        return Ok(());
                    }
                }

                println!("Closing connection with token {:?}", token);
                self.cancel_connection(&token)?;
                return Ok(());
            }
            TIMER => self.handle_timer()?,
            _ => unreachable!(),
        }

        Ok(())
    }

    /// Runs the server's event loop.
    pub fn run(&mut self) -> Result<()> {
        let mut events = Events::with_capacity(1024);
        loop {
            self.poll.poll(&mut events, None)?;

            for event in events.iter() {
                self.handle_token(event.token())?;
            }
        }
    }

    /// Returns the socket address of the server socket.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }
}

/// Creates a std::net::UdpSocket on a random open UDP port.
/// The range of valid ports is from 0 to 65535 and if the function
/// cannot find a open port within 100 different random ports it returns an error.
pub fn create_socket(timeout: Option<Duration>) -> Result<net::UdpSocket> {
    let mut num_failures = 0;
    let mut past_ports = HashMap::new();
    loop {
        let port = rand::thread_rng().gen_range(0, 65535);
        // Ignore ports that already failed.
        if past_ports.get(&port).is_some() {
            continue;
        }

        let addr = format!("127.0.0.1:{}", port);
        let socket_addr = SocketAddr::from_str(addr.as_str()).expect("Error parsing address");
        match net::UdpSocket::bind(&socket_addr) {
            Ok(socket) => {
                if let Some(timeout) = timeout {
                    socket.set_read_timeout(Some(timeout))?;
                    socket.set_write_timeout(Some(timeout))?;
                }
                return Ok(socket);
            }
            Err(_) => {
                past_ports.insert(port, true);
                num_failures += 1;
                if num_failures > 100 {
                    return Err(TftpError::NoOpenSocket);
                }
            }
        }
    }
}

/// Increments the block number and handles wraparound to 0 instead of overflow.
pub fn incr_block_num(block_num: &mut u16) {
    if *block_num == u16::MAX - 1 {
        *block_num = 0;
    } else {
        *block_num += 1;
    }
}

fn handle_rrq_packet(filename: String, mode: String) -> Result<(File, u16, Packet)> {
    println!("Received RRQ packet with filename {} and mode {}",
             filename,
             mode);
    let mut file = File::open(filename)?;
    let block_num = 1;

    let mut buf = [0; 512];
    let amount = file.read(&mut buf)?;

    // Reply with first data packet with a block number of 1
    let last_packet = Packet::DATA {
        block_num: block_num,
        data: DataBytes(buf),
        len: amount,
    };

    Ok((file, block_num, last_packet))
}

fn handle_wrq_packet(filename: String, mode: String) -> Result<(File, u16, Packet)> {
    println!("Received WRQ packet with filename {} and mode {}",
             filename,
             mode);
    let file = File::create(filename)?;
    let block_num = 0;

    // Reply with ACK with a block number of 0
    let last_packet = Packet::ACK(block_num);

    Ok((file, block_num, last_packet))
}

fn handle_ack_packet(block_num: u16, conn: &mut ConnectionState) -> Result<()> {
    println!("Received ACK with block number {}", block_num);
    if block_num != conn.block_num {
        return Ok(());
    }

    incr_block_num(&mut conn.block_num);
    let mut buf = [0; 512];
    let amount = conn.file.read(&mut buf)?;

    // Send next data packet
    conn.last_packet = Packet::DATA {
        block_num: conn.block_num,
        data: DataBytes(buf),
        len: amount,
    };
    conn.conn.send_to(conn.last_packet.clone().bytes()?.to_slice(), &conn.addr)?;

    if amount < 512 {
        Err(TftpError::CloseConnection)
    } else {
        Ok(())
    }
}

fn handle_data_packet(block_num: u16,
                      data: DataBytes,
                      len: usize,
                      conn: &mut ConnectionState)
                      -> Result<()> {
    println!("Received data with block number {}", block_num);

    incr_block_num(&mut conn.block_num);
    if block_num < conn.block_num {
        return Ok(());
    }

    conn.file.write(&data.0[0..len])?;

    // Send ACK packet for data
    conn.last_packet = Packet::ACK(conn.block_num);
    conn.conn.send_to(conn.last_packet.clone().bytes()?.to_slice(), &conn.addr)?;

    if len < 512 {
        Err(TftpError::CloseConnection)
    } else {
        Ok(())
    }
}
