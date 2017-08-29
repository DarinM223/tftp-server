use mio::*;
use mio_more::timer::{Timer, TimerError, Timeout};
use mio::net::UdpSocket;
use packet::{ErrorCode, MAX_PACKET_SIZE, Packet, PacketErr};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{self, SocketAddr, IpAddr};
use std::result;
use std::time::Duration;
use tftp_proto::*;

/// The token used by the timer.
const TIMER: Token = Token(0);

#[derive(Debug)]
pub enum TftpError {
    PacketError(PacketErr),
    IoError(io::Error),
    TimerError(TimerError),
    /// Error defined within the TFTP spec with an usigned integer
    /// error code. The server should reply with an error packet
    /// to the given socket address when handling this error.
    TftpError(ErrorCode, SocketAddr),
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

/// Trait used to inject filesystem IO handling into a server.
/// A trivial default implementation is provided by `FSAdapter`.
/// If you want to employ things like buffered IO, it can be done by providing
/// an implementation for this trait and passing the implementing type to the server.
pub trait IOAdapter {
    type R: Read + Sized;
    type W: Write + Sized;
    fn open_read(&self, filename: &str) -> io::Result<Self::R>;
    fn create_new(&mut self, filename: &str) -> io::Result<Self::W>;
}

/// Provides a simple, default implementation for `IOAdapter`.
pub struct FSAdapter;

impl IOAdapter for FSAdapter {
    type R = File;
    type W = File;
    fn open_read(&self, filename: &str) -> io::Result<File> {
        File::open(filename)
    }
    fn create_new(&mut self, filename: &str) -> io::Result<File> {
        fs::OpenOptions::new().write(true).create_new(true).open(
            filename,
        )
    }
}

impl Default for FSAdapter {
    fn default() -> Self {
        FSAdapter
    }
}

/// The state contained within a connection.
/// A connection is started when a server socket receives
/// a RRQ or a WRQ packet and ends when the connection socket
/// receives a DATA packet less than 516 bytes or if the connection
/// socket receives an invalid packet.
struct ConnectionState<IO: IOAdapter> {
    /// The UDP socket for the connection that receives ACK, DATA, or ERROR packets.
    socket: UdpSocket,
    /// The timeout for the last packet. Every time a new packet is received, the
    /// timeout is reset.
    timeout: Timeout,
    /// The protocol state associated with this transfer
    transfer: Transfer<IO>,
    /// The last packet sent. This is used when a timeout happens to resend the last packet.
    last_packet: Packet,
    /// The address of the client socket to reply to.
    remote: SocketAddr,
}

/// Struct used to specify working configuration of a server
pub struct ServerConfig {
    /// Specifies that the server should reject write requests
    pub readonly: bool,
    /// The directory the server will serve from instead of the default
    pub dir: Option<String>,
    /// The IP addresses (and optionally ports) on which the server must listen
    pub addrs: Vec<(IpAddr, Option<u16>)>,
    /// The idle time until a connection with a client is closed
    pub timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            readonly: false,
            dir: None,
            addrs: vec![(IpAddr::from([127, 0, 0, 1]), None)],
            timeout: Duration::from_secs(3),
        }
    }
}

pub type TftpServer = TftpServerImpl<FSAdapter>;

pub struct TftpServerImpl<IO: IOAdapter> {
    /// The ID of a new token used for generating different tokens.
    new_token: Token,
    /// The event loop for handling async events.
    poll: Poll,
    /// The main timer that can be used to set multiple timeout events.
    timer: Timer<Token>,
    /// The connection timeout
    timeout: Duration,
    /// The main server socket that receives RRQ and WRQ packets
    /// and creates a new separate UDP connection.
    server_sockets: HashMap<Token, UdpSocket>,
    /// The separate UDP connections for handling multiple requests.
    connections: HashMap<Token, ConnectionState<IO>>,
    /// The TFTP protocol state machine and filesystem accessor
    proto_handler: TftpServerProto<IO>,
}

impl<IO: IOAdapter + Default> TftpServerImpl<IO> {
    /// Creates a new TFTP server from a random open UDP port.
    pub fn new() -> Result<Self> {
        Self::with_cfg(&Default::default())
    }

    /// Creates a new TFTP server from the provided config
    pub fn with_cfg(cfg: &ServerConfig) -> Result<Self> {
        if cfg.addrs.is_empty() {
            return Err(TftpError::IoError(io::Error::new(
                io::ErrorKind::InvalidInput,
                "address list empty; nothing to listen on",
            )));
        }

        let poll = Poll::new()?;
        let timer = Timer::default();
        poll.register(
            &timer,
            TIMER,
            Ready::readable(),
            PollOpt::edge() | PollOpt::level(),
        )?;

        let mut server_sockets = HashMap::new();
        let mut new_token = Token(1); // skip timer token
        for addr in &cfg.addrs {
            let socket = make_bound_socket(addr.0, addr.1)?;
            poll.register(
                &socket,
                new_token,
                Ready::readable(),
                PollOpt::edge() | PollOpt::level(),
            )?;
            server_sockets.insert(new_token, socket);
            new_token.0 += 1;
        }

        info!(
            "Server listening on {:?}",
            server_sockets
                .iter()
                .map(|(_, socket)| {
                    format!("{}", socket.local_addr().unwrap())
                })
                .collect::<Vec<_>>()
        );

        Ok(Self {
            new_token,
            poll,
            timer,
            timeout: cfg.timeout,
            server_sockets,
            connections: HashMap::new(),
            proto_handler: TftpServerProto::new(
                Default::default(),
                IOPolicyCfg {
                    readonly: cfg.readonly,
                    path: cfg.dir.clone(),
                },
            ),
        })
    }

    /// Returns a new token created from incrementing a counter.
    fn generate_token(&mut self) -> Token {
        use std::usize;
        if self.connections
            .len()
            .saturating_add(self.server_sockets.len())
            .saturating_add(1 /* timer token */) == usize::MAX
        {
            panic!("no more tokens, but impressive amount of memory");
        }
        while self.new_token == TIMER || self.server_sockets.contains_key(&self.new_token) ||
            self.connections.contains_key(&self.new_token)
        {
            self.new_token.0 = self.new_token.0.wrapping_add(1);
        }
        self.new_token
    }

    /// Cancels a connection given the connection's token. It cancels the
    /// connection's timeout and deregisters the connection's socket from the event loop.
    fn cancel_connection(&mut self, token: &Token) -> Result<()> {
        if let Some(conn) = self.connections.remove(token) {
            info!("Closing connection with token {:?}", token);
            self.poll.deregister(&conn.socket)?;
            self.timer.cancel_timeout(&conn.timeout);
        }
        Ok(())
    }

    /// Resets a connection's timeout given the connection's token.
    fn reset_timeout(&mut self, token: &Token) -> Result<()> {
        if let Some(ref mut conn) = self.connections.get_mut(token) {
            self.timer.cancel_timeout(&conn.timeout);
            conn.timeout = self.timer.set_timeout(self.timeout, *token)?;
        }
        Ok(())
    }

    /// Creates a new UDP connection from the provided arguments
    fn create_connection(
        &mut self,
        token: Token,
        socket: UdpSocket,
        transfer: Transfer<IO>,
        packet: Packet,
        remote: SocketAddr,
    ) -> Result<()> {
        let timeout = self.timer.set_timeout(self.timeout, token)?;
        self.poll.register(
            &socket,
            token,
            Ready::readable(),
            PollOpt::edge() | PollOpt::level(),
        )?;

        self.connections.insert(
            token,
            ConnectionState {
                socket,
                timeout,
                transfer,
                last_packet: packet,
                remote,
            },
        );

        info!("Created connection with token: {:?}", token);

        Ok(())
    }

    /// Handles the event when a timer times out.
    /// It gets the connection from the token and resends
    /// the last packet sent from the connection.
    /// If the transfer associated with that connection is over,
    /// it instead kills the connection.
    fn process_timer(&mut self) -> Result<()> {
        let mut tokens = Vec::new();
        while let Some(token) = self.timer.poll() {
            tokens.push(token);
        }

        for token in tokens {
            if Some(true) ==
                self.connections.get(&token).map(
                    |conn| conn.transfer.is_done(),
                )
            {
                self.cancel_connection(&token)?;
            } else if let Some(ref mut conn) = self.connections.get_mut(&token) {
                conn.socket.send_to(
                    conn.last_packet.to_bytes()?.to_slice(),
                    &conn.remote,
                )?;
            }
            self.reset_timeout(&token)?;
        }

        Ok(())
    }

    /// Called to process an available I/O event for a token.
    /// Normally these correspond to packets received on a socket or to a timeout
    fn handle_token(&mut self, token: Token, mut buf: &mut [u8]) -> Result<()> {
        match token {
            TIMER => self.process_timer(),
            _ if self.server_sockets.contains_key(&token) => {
                self.handle_server_packet(token, &mut buf)
            }
            _ => self.handle_connection_packet(token, &mut buf),
        }
    }

    fn handle_server_packet(&mut self, token: Token, mut buf: &mut [u8]) -> Result<()> {
        let (local_ip, amt, src) = {
            let socket = match self.server_sockets.get(&token) {
                Some(socket) => socket,
                None => {
                    error!("Invalid server token");
                    return Ok(());
                }
            };
            let (amt, src) = socket.recv_from(&mut buf)?;
            (socket.local_addr()?.ip(), amt, src)
        };
        let packet = Packet::read(&buf[..amt])?;

        let new_conn_token = self.generate_token();
        let (xfer, res) = self.proto_handler.rx_initial(packet);
        let reply_packet = match res {
            Err(e) => {
                error!("{:?}", e);
                return Ok(());
            }
            Ok(packet) => packet,
        };

        let socket = make_bound_socket(local_ip, None)?;

        // send packet back for all cases
        socket.send_to(reply_packet.to_bytes()?.to_slice(), &src)?;

        if let Some(xfer) = xfer {
            self.create_connection(
                new_conn_token,
                socket,
                xfer,
                reply_packet,
                src,
            )?;
        }

        Ok(())
    }

    fn handle_connection_packet(&mut self, token: Token, mut buf: &mut [u8]) -> Result<()> {
        use self::TftpResult::*;

        self.reset_timeout(&token)?;
        let conn = match self.connections.get_mut(&token) {
            Some(conn) => conn,
            None => {
                error!("No connection with token {:?}", token);
                return Ok(());
            }
        };
        let (amt, src) = conn.socket.recv_from(&mut buf)?;

        if conn.remote != src {
            // packet from somehere else, reply with error
            conn.socket.send_to(
                Packet::ERROR {
                    code: ErrorCode::UnknownID,
                    msg: "".to_owned(),
                }.into_bytes()?
                    .to_slice(),
                &conn.remote,
            )?;
            return Ok(());
        }
        let packet = Packet::read(&buf[..amt])?;

        let response = match conn.transfer.rx(packet) {
            Err(e) => {
                error!("{:?}", e);
                None
            }
            Repeat => Some(&conn.last_packet),
            Reply(packet) |
            Done(Some(packet)) => {
                conn.last_packet = packet;
                Some(&conn.last_packet)
            }
            Done(None) => None,
        };

        if let Some(packet) = response {
            conn.socket.send_to(
                packet.to_bytes()?.to_slice(),
                &conn.remote,
            )?;
        }
        Ok(())
    }

    /// Runs the server's event loop.
    pub fn run(&mut self) -> Result<()> {
        let mut events = Events::with_capacity(1024);
        let mut scratch_buf = [0; MAX_PACKET_SIZE];

        loop {
            self.poll.poll(&mut events, None)?;

            for event in events.iter() {
                match self.handle_token(event.token(), &mut scratch_buf) {
                    Ok(_) |
                    Err(TftpError::IoError(_)) => { /* swallow Io errors */ }
                    Err(TftpError::PacketError(_)) => {
                        error!("malformed packet");
                    }
                    e => return e,
                }
            }
        }
    }

    /// Stores the local addresses in the provided vec
    pub fn get_local_addrs(&self, bag: &mut Vec<SocketAddr>) -> Result<()> {
        for socket in self.server_sockets.values() {
            bag.push(socket.local_addr()?);
        }
        Ok(())
    }
}

fn make_bound_socket(ip: IpAddr, port: Option<u16>) -> Result<UdpSocket> {
    let socket = net::UdpSocket::bind((ip, port.unwrap_or(0)))?;

    socket.set_nonblocking(true)?;

    Ok(UdpSocket::from_socket(socket)?)
}
