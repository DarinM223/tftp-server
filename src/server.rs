use mio::*;
use mio::timer::{Timer, TimerError, Timeout};
use mio::udp::UdpSocket;
use packet::{ErrorCode, MAX_PACKET_SIZE, DataBytes, Packet, PacketData, PacketErr};
use rand;
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use std::net;
use std::net::SocketAddr;
use std::result;
use std::str::FromStr;
use std::time::Duration;
use std::u16;

/// Timeout time until packet is re-sent.
const TIMEOUT: u64 = 3;
/// The token used by the server UDP socket.
const SERVER: Token = Token(0);
/// The token used by the timer.
const TIMER: Token = Token(1);

#[derive(Debug)]
pub enum TftpError {
    PacketError(PacketErr),
    IoError(io::Error),
    TimerError(TimerError),
    /// Error defined within the TFTP spec with an usigned integer
    /// error code. The server should reply with an error packet
    /// to the given socket address when handling this error.
    TftpError(ErrorCode, SocketAddr),
    /// Error returned when the server cannot
    /// find a random open UDP port within 100 tries.
    NoOpenSocket,
    /// Error signalling that a received packed did not match
    /// the current expected block number
    BlockNoMismatch,
    /// Error returned when a read connection receives a DATA packet
    /// or a write connection receives an ACK
    WrongPacketType,
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

trait IOAdapter {
    type R: Read + Sized;
    type W: Write + Sized;
    fn open_read(filename: String) -> io::Result<Self::R>;
    fn create_new(filename: String) -> io::Result<Self::W>;
}

struct FSAdapter;

impl IOAdapter for FSAdapter {
    type R = File;
    type W = File;
    fn open_read(filename: String) -> io::Result<File> {
        File::open(filename)
    }
    fn create_new(filename: String) -> io::Result<File> {
        fs::OpenOptions::new().write(true).create_new(true).open(
            filename,
        )
    }
}

type ConnectionState = ConnectionStateImpl<FSAdapter>;

enum RW<IO: IOAdapter> {
    R(IO::R),
    W(IO::W),
}

/// The state contained within a connection.
/// A connection is started when a server socket receives
/// a RRQ or a WRQ packet and ends when the connection socket
/// receives a DATA packet less than 516 bytes or if the connection
/// socket receives an invalid packet.
struct ConnectionStateImpl<IO: IOAdapter> {
    /// The UDP socket for the connection that receives ACK, DATA, or ERROR packets.
    conn: UdpSocket,
    /// The open file either being written to or read from during the transfer.
    /// If the connection was started with a RRQ, the file would be read from, if it
    /// was started with a WRQ, the file would be written to.
    file: RW<IO>,
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
    /// Indicates the transfer has completed, and the next timeout will close the connection
    dallying: bool,
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
        poll.register(
            &socket,
            SERVER,
            Ready::readable() | Ready::writable(),
            PollOpt::edge(),
        )?;
        poll.register(
            &timer,
            TIMER,
            Ready::readable(),
            PollOpt::edge(),
        )?;

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
        poll.register(
            &socket,
            SERVER,
            Ready::readable() | Ready::writable(),
            PollOpt::edge(),
        )?;
        poll.register(
            &timer,
            TIMER,
            Ready::readable(),
            PollOpt::edge(),
        )?;

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

    /// Cancels a connection given the connection's token. It cancels the
    /// connection's timeout and deregisters the connection's socket from the event loop.
    fn cancel_connection(&mut self, token: &Token) -> Result<()> {
        if let Some(conn) = self.connections.remove(token) {
            info!("Closing connection with token {:?}", token);
            self.poll.deregister(&conn.conn)?;
            self.timer.cancel_timeout(&conn.timeout);
        }
        Ok(())
    }

    /// Resets a connection's timeout given the connection's token.
    fn reset_timeout(&mut self, token: &Token) -> Result<()> {
        if let Some(ref mut conn) = self.connections.get_mut(token) {
            self.timer.cancel_timeout(&conn.timeout);
            conn.timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT), *token)?;
        }
        Ok(())
    }

    /// Handles a packet sent to the main server connection.
    /// It opens a new UDP connection in a random port and replies with either an ACK
    /// or a DATA packet depending on the whether it received an RRQ or a WRQ packet.
    fn handle_server_packet(
        &mut self,
        packet: Packet,
        src: &SocketAddr,
    ) -> Result<(Packet, Token)> {
        // Handle the RRQ or WRQ packet.
        let (io_handle, block_num, send_packet) = match packet {
            Packet::RRQ { filename, mode } => ConnectionState::new_from_rrq(filename, mode, src)?,
            Packet::WRQ { filename, mode } => ConnectionState::new_from_wrq(filename, mode, src)?,
            _ => return Err(TftpError::TftpError(ErrorCode::IllegalTFTP, *src)),
        };

        // Create new connection.
        let socket = UdpSocket::from_socket(create_socket(Some(Duration::from_secs(TIMEOUT)))?)?;
        let token = self.generate_token();
        let timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT), token)?;
        self.poll.register(
            &socket,
            token,
            Ready::readable() | Ready::writable(),
            PollOpt::edge(),
        )?;
        info!("Created connection with token: {:?}", token);

        self.connections.insert(
            token,
            ConnectionState {
                conn: socket,
                file: io_handle,
                timeout: timeout,
                block_num: block_num,
                last_packet: send_packet.clone(),
                addr: *src,
                dallying: false,
            },
        );

        Ok((send_packet, token))
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
            if Some(true) == self.connections.get(&token).map(|conn| conn.dallying) {
                self.cancel_connection(&token)?;
            } else if let Some(ref mut conn) = self.connections.get_mut(&token) {
                info!("Timeout: resending last packet for token: {:?}", token);
                conn.conn.send_to(
                    conn.last_packet.clone().to_bytes()?.to_slice(),
                    &conn.addr,
                )?;
            }
            self.reset_timeout(&token)?;
        }

        Ok(())
    }

    /// Handles a packet sent to an open child connection.
    fn handle_connection_packet(&mut self, packet: Packet, token: Token) -> Result<Packet> {
        if let Some(ref mut conn) = self.connections.get_mut(&token) {
            match packet {
                Packet::ACK(block_num) => Ok(conn.handle_ack_packet(block_num)?),
                Packet::DATA {
                    block_num,
                    data,
                    len,
                } => Ok(conn.handle_data_packet(block_num, data, len)?),
                Packet::ERROR { code, msg } => {
                    error!("Error message received with code {:?}: {:?}", code, msg);
                    Err(TftpError::TftpError(code, conn.addr))
                }
                _ => {
                    error!("Received invalid packet from connection");
                    Err(TftpError::TftpError(ErrorCode::IllegalTFTP, conn.addr))
                }
            }
        } else {
            Err(TftpError::NoOpenSocket)
        }
    }

    /// Handles sending error packets given the error code.
    fn handle_error(&mut self, token: &Token, code: ErrorCode, addr: &SocketAddr) -> Result<()> {
        if *token == SERVER {
            self.socket.send_to(
                code.to_packet().to_bytes()?.to_slice(),
                addr,
            )?;
        } else if let Some(ref mut conn) = self.connections.get_mut(&token) {
            conn.conn.send_to(
                code.to_packet().to_bytes()?.to_slice(),
                addr,
            )?;
        }
        Ok(())
    }

    /// Called for every event sent from the event loop. The event
    /// is a token that can either be from the server, from an open connection,
    /// or from a timeout timer for a connection.
    pub fn handle_token(&mut self, token: Token) -> Result<()> {
        let mut buf = [0; MAX_PACKET_SIZE];
        match token {
            TIMER => self.handle_timer()?,
            SERVER => {
                let (amt, src) = match self.socket.recv_from(&mut buf)? {
                    Some((amt, src)) => (amt, src),
                    None => return Ok(()),
                };
                let packet = Packet::read(PacketData::new(buf, amt))?;

                match self.handle_server_packet(packet, &src) {
                    Ok((pkt, token)) => {
                        self.connections.get_mut(&token).unwrap().conn.send_to(
                            pkt.clone().to_bytes()?.to_slice(),
                            &src,
                        )?;
                    }
                    Err(TftpError::TftpError(code, addr)) => {
                        self.handle_error(&token, code, &addr)?
                    }
                    Err(e) => error!("Error: {:?}", e),
                }
            }
            _ => {
                let packet;
                {
                    let conn = self.connections.get_mut(&token).unwrap();
                    let amt = match conn.conn.recv_from(&mut buf)? {
                        Some((amt, _)) => amt,
                        None => return Ok(()),
                    };
                    packet = Packet::read(PacketData::new(buf, amt))?;
                }

                match self.handle_connection_packet(packet, token) {
                    Ok(pkt) => {
                        {
                            // TODO: fix clumsy repeated lookup of connections
                            let conn = self.connections.get_mut(&token).unwrap();
                            conn.conn.send_to(
                                pkt.clone().to_bytes()?.to_slice(),
                                &conn.addr,
                            )?;
                        }
                        self.reset_timeout(&token)?;
                        return Ok(());
                    }
                    Err(TftpError::TftpError(code, addr)) => {
                        self.handle_error(&token, code, &addr)?
                    }
                    Err(e) => error!("Error: {:?}", e),
                }

                self.cancel_connection(&token)?;
                return Ok(());
            }
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
    let mut failed_ports = HashSet::new();
    for _ in 0..100 {
        let port = rand::thread_rng().gen_range(0, 65535);
        if failed_ports.contains(&port) {
            continue;
        }

        let addr = format!("127.0.0.1:{}", port);
        let socket_addr = SocketAddr::from_str(addr.as_str()).expect("Error parsing address");
        if let Ok(socket) = net::UdpSocket::bind(&socket_addr) {
            if let Some(timeout) = timeout {
                socket.set_read_timeout(Some(timeout))?;
                socket.set_write_timeout(Some(timeout))?;
            }
            return Ok(socket);
        }
        failed_ports.insert(port);
    }

    Err(TftpError::NoOpenSocket)
}

impl<IO: IOAdapter> ConnectionStateImpl<IO> {
fn new_from_rrq(
    filename: String,
    mode: String,
    addr: &SocketAddr,
) -> Result<(RW<IO>, u16, Packet)> {
    info!(
        "Received RRQ packet with filename {} and mode {}",
        filename,
        mode
    );

    if filename.contains("..") || filename.starts_with('/') {
        return Err(TftpError::TftpError(ErrorCode::FileNotFound, *addr));
    }

    let mut file = IO::open_read(filename).map_err(|_| {
        TftpError::TftpError(ErrorCode::FileNotFound, *addr)
    })?;
    let block_num = 1;

    let mut buf = [0; 512];
    let amount = file.read(&mut buf)?;

    // Reply with first data packet with a block number of 1.
    let last_packet = Packet::DATA {
        block_num: block_num,
        data: DataBytes(buf),
        len: amount,
    };

    Ok((RW::R(file), block_num, last_packet))
}

fn new_from_wrq(
    filename: String,
    mode: String,
    addr: &SocketAddr,
) -> Result<(RW<IO>, u16, Packet)> {
    info!(
        "Received WRQ packet with filename {} and mode {}",
        filename,
        mode
    );
    let file = IO::create_new(filename).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => TftpError::TftpError(ErrorCode::FileExists, *addr),
        _ => e.into(),
    })?;
    let block_num = 0;

    // Reply with ACK with a block number of 0.
    let last_packet = Packet::ACK(block_num);

    Ok((RW::W(file), block_num, last_packet))
}

fn handle_ack_packet(&mut self, block_num: u16) -> Result<Packet> {
    info!("Received ACK with block number {}", block_num);
    if block_num != self.block_num {
        return Err(TftpError::BlockNoMismatch);
    }

    self.block_num = self.block_num.wrapping_add(1);
    let mut buf = [0; 512];
    let amount = match self.file {
        RW::R(ref mut r) => r.read(&mut buf)?,
        _ => return Err(TftpError::WrongPacketType),
    };

    // Send next data packet.
    self.last_packet = Packet::DATA {
        block_num: self.block_num,
        data: DataBytes(buf),
        len: amount,
    };

    if amount < 512 {
        self.dallying = true;
    }

    Ok(self.last_packet.clone())
}

fn handle_data_packet(
    &mut self,
    block_num: u16,
    data: DataBytes,
    len: usize,
) -> Result<Packet> {
    info!("Received data with block number {}", block_num);

    self.block_num = self.block_num.wrapping_add(1);
    if block_num != self.block_num {
        return Err(TftpError::BlockNoMismatch);
    }

    match self.file {
        RW::W(ref mut w) => w.write_all(&data.0[0..len])?,
        _ => return Err(TftpError::WrongPacketType),
    }

    self.last_packet = Packet::ACK(self.block_num);

    if len < 512 {
        self.dallying = true;
    }

    Ok(self.last_packet.clone())
}
}
