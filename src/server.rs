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

const TIMEOUT_LENGTH: u64 = 5;
const SERVER: Token = Token(0);
const TIMER: Token = Token(1);

#[derive(Debug)]
pub enum TftpError {
    PacketError(PacketErr),
    IoError(io::Error),
    TimerError(TimerError),
    NoOpenSocket,
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
        let socket = UdpSocket::from_socket(create_socket(Duration::from_secs(TIMEOUT_LENGTH))?)?;
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

    /// Handles a packet sent to the main server connection.
    /// It opens a new UDP connection in a random port and replies with either an ACK
    /// or a DATA packet depending on the whether it received an RRQ or a WRQ packet.
    fn handle_server_packet(&mut self) -> Result<bool> {
        let mut buf = [0; MAX_PACKET_SIZE];
        let (amt, src) = match self.socket.recv_from(&mut buf)? {
            Some((amt, src)) => (amt, src),
            None => {
                println!("Getting None when receiving from server socket");
                return Ok(false);
            }
        };
        let packet = Packet::read(PacketData::new(buf, amt))?;

        // Only allow RRQ and WRQ packets to be received
        match packet {
            Packet::RRQ { .. } |
            Packet::WRQ { .. } => {}
            _ => {
                println!("Error: Received invalid packet");
                return Ok(false);
            }
        }

        let socket = UdpSocket::from_socket(create_socket(Duration::from_secs(TIMEOUT_LENGTH))?)?;
        let token = self.generate_token();

        self.poll.register(&socket, token, Ready::all(), PollOpt::edge())?;

        let timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT_LENGTH), token)?;

        // Handle the RRQ or WRQ packet
        let (file, block_num, send_packet) = match packet {
            Packet::RRQ { filename, mode } => handle_rrq_packet(filename, mode)?,
            Packet::WRQ { filename, mode } => handle_wrq_packet(filename, mode)?,
            _ => unreachable!(),
        };

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

        Ok(false)
    }

    /// Handles the event when a timer times out.
    /// It gets the connection from the token and resends
    /// the last packet sent from the connection.
    fn handle_timer(&mut self) -> Result<bool> {
        let token = match self.timer.poll() {
            Some(token) => token,
            None => return Ok(false),
        };
        if let Some(ref mut conn) = self.connections.get_mut(&token) {
            println!("Timeout: resending last packet");
            conn.conn.send_to(conn.last_packet.clone().bytes()?.to_slice(), &conn.addr)?;
        }

        Ok(false)
    }

    /// Handles a packet sent to an open child connection.
    fn handle_connection_packet(&mut self, token: Token) -> Result<bool> {
        if let Some(mut conn) = self.connections.remove(&token) {
            let mut buf = [0; MAX_PACKET_SIZE];
            let amt = match conn.conn.recv_from(&mut buf)? {
                Some((amt, _)) => amt,
                None => {
                    println!("Getting None when receiving from connection socket");
                    return Ok(false);
                }
            };
            let packet = Packet::read(PacketData::new(buf, amt))?;

            let close_connection = match packet {
                Packet::ACK(block_num) => handle_ack_packet(block_num, &mut conn)?,
                Packet::DATA { block_num, data, len } => {
                    handle_data_packet(block_num, data, len, &mut conn)?
                }
                Packet::ERROR { .. } => {
                    println!("Error message received");
                    true
                }
                _ => {
                    println!("Received invalid packet from connection");
                    true
                }
            };

            assert!(self.timer.cancel_timeout(&conn.timeout).is_some());
            if close_connection {
                self.poll.deregister(&conn.conn)?;
            } else {
                // Reset timeout
                conn.timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT_LENGTH), token)?;

                // Reinsert the connection if connection is not to be closed
                self.connections.insert(token, conn);
            }
        }

        Ok(false)
    }

    /// Runs the server's event loop.
    pub fn run(&mut self) -> Result<()> {
        let mut events = Events::with_capacity(1024);
        'main_loop: loop {
            self.poll.poll(&mut events, None)?;

            for event in events.iter() {
                let finished = match event.token() {
                    SERVER => self.handle_server_packet()?,
                    TIMER => self.handle_timer()?,
                    token if self.connections.get(&token).is_some() => {
                        self.handle_connection_packet(token)?
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

    /// Returns the socket address of the server socket.
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.socket.local_addr()?)
    }
}

/// Creates a std::net::UdpSocket on a random open UDP port.
/// The range of valid ports is from 0 to 65535 and if the function
/// cannot find a open port within 100 different random ports it returns an error.
pub fn create_socket(timeout: Duration) -> Result<net::UdpSocket> {
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
                socket.set_read_timeout(Some(timeout))?;
                socket.set_write_timeout(Some(timeout))?;
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

fn handle_ack_packet(block_num: u16, conn: &mut ConnectionState) -> Result<bool> {
    println!("Received ACK with block number {}", block_num);
    if block_num != conn.block_num {
        return Ok(true);
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

    Ok(false)
}

fn handle_data_packet(block_num: u16,
                      data: DataBytes,
                      len: usize,
                      conn: &mut ConnectionState)
                      -> Result<bool> {
    println!("Received data with block number {}", block_num);

    incr_block_num(&mut conn.block_num);
    if block_num != conn.block_num {
        return Ok(true);
    }

    conn.file.write(&data.0[0..len])?;

    // Send ACK packet for data
    conn.last_packet = Packet::ACK(conn.block_num);
    conn.conn.send_to(conn.last_packet.clone().bytes()?.to_slice(), &conn.addr)?;

    Ok(len < 512)
}
