use mio::*;
use mio_more::timer::{Timer, TimerError, Timeout};
use mio::udp::UdpSocket;
use packet::{ErrorCode, MAX_PACKET_SIZE, Packet, PacketErr};
use rand::{self, Rng};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{self, SocketAddr};
use std::result;
use std::str::FromStr;
use std::time::Duration;
use tftp_proto::*;

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

    transfer: Transfer<IO>,
    /// The last packet sent. This is used when a timeout happens to resend the last packet.
    last_packet: Packet,
    /// The address of the client socket to reply to.
    remote: SocketAddr,
    /// Indicates the transfer has completed, and the next timeout will close the connection
    dallying: bool,
}

pub type TftpServer = TftpServerImpl<FSAdapter>;

pub struct TftpServerImpl<IO: IOAdapter> {
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
    connections: HashMap<Token, ConnectionState<IO>>,
    /// The TFTP protocol state machine and filesystem accessor
    proto_handler: TftpServerProto<IO>,
}

impl<IO: IOAdapter + Default> TftpServerImpl<IO> {
    /// Creates a new TFTP server from a random open UDP port.
    pub fn new() -> Result<Self> {
        Self::new_from_socket(UdpSocket::from_socket(
            create_socket(Some(Duration::from_secs(TIMEOUT)))?,
        )?)
    }

    /// Creates a new TFTP server from a socket address.
    pub fn new_from_addr(addr: &SocketAddr) -> Result<Self> {
        Self::new_from_socket(UdpSocket::bind(addr)?)
    }

    fn new_from_socket(socket: UdpSocket) -> Result<Self> {
        let poll = Poll::new()?;
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

        Ok(Self {
            new_token: 2,
            poll: poll,
            timer: timer,
            socket: socket,
            connections: HashMap::new(),
            proto_handler: TftpServerProto::new(Default::default()),
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
            self.poll.deregister(&conn.socket)?;
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
    fn create_connection(
        &mut self,
        token: Token,
        transfer: Transfer<IO>,
        packet: Packet,
        remote: SocketAddr,
    ) -> Result<()> {
        let socket = UdpSocket::from_socket(create_socket(Some(Duration::from_secs(TIMEOUT)))?)?;
        let timeout = self.timer.set_timeout(Duration::from_secs(TIMEOUT), token)?;
        self.poll.register(
            &socket,
            token,
            Ready::readable() | Ready::writable(),
            PollOpt::edge(),
        )?;
        info!("Created connection with token: {:?}", token);

        socket.send_to(packet.to_bytes()?.to_slice(), &remote)?;

        self.connections.insert(
            token,
            ConnectionState {
                socket,
                timeout,
                transfer,
                last_packet: packet,
                remote,
                dallying: false,
            },
        );

        Ok(())
    }

    /// Handles the event when a timer times out.
    /// It gets the connection from the token and resends
    /// the last packet sent from the connection.
    fn process_timer(&mut self) -> Result<()> {
        let mut tokens = Vec::new();
        while let Some(token) = self.timer.poll() {
            tokens.push(token);
        }

        for token in tokens {
            if Some(true) == self.connections.get(&token).map(|conn| conn.dallying) {
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

    /// Called for every event sent from the event loop. The event
    /// is a token that can either be from the server, from an open connection,
    /// or from a timeout timer for a connection.
    fn handle_token(&mut self, token: Token) -> Result<()> {
        match token {
            TIMER => self.process_timer(),
            SERVER => self.handle_server_packet(),
            _ => self.handle_connection_packet(token),
        }
    }

    fn handle_server_packet(&mut self) -> Result<()> {
        use self::TftpResult::*;
        let mut buf = [0; MAX_PACKET_SIZE];

        let (amt, src) = match self.socket.recv_from(&mut buf)? {
            Some((amt, src)) => (amt, src),
            None => return Ok(()),
        };
        let packet = Packet::read(&buf[..amt])?;

        let new_conn_token = self.generate_token();
        let (xfer, res) = self.proto_handler.rx_initial(packet);
        match res {
            Err(e) => error!("{:?}", e),
            Repeat => error!("cannot handle repeat of nothing"),
            Reply(packet) => {
                return self.create_connection(new_conn_token, xfer.unwrap(), packet, src);
            }
            Done(Some(packet)) => {
                let socket = create_socket(None)?;
                socket.send_to(packet.into_bytes()?.to_slice(), src)?;
            }
            Done(None) => {}
        }
        Ok(())
    }

    fn handle_connection_packet(&mut self, token: Token) -> Result<()> {
        use self::TftpResult::*;
        let mut buf = [0; MAX_PACKET_SIZE];

        self.reset_timeout(&token)?;
        let conn = match self.connections.get_mut(&token) {
            Some(conn) => conn,
            None => {
                error!("No connection with token {:?}", token);
                return Ok(());
            }
        };
        let (amt, src) = match conn.socket.recv_from(&mut buf)? {
            Some((amt, src)) => (amt, src),
            None => return Ok(()),
        };

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
            Reply(packet) => {
                conn.last_packet = packet;
                Some(&conn.last_packet)
            }
            Done(response) => {
                conn.dallying = true;
                if let Some(packet) = response {
                    conn.last_packet = packet;
                    Some(&conn.last_packet)
                } else {
                    None
                }
            }
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

/// Creates a `std::net::UdpSocket` on a random open UDP port.
/// The range of valid ports is from 0 to 65535 and if the function
/// cannot find a open port within 100 different random ports it returns an error.
pub fn create_socket(timeout: Option<Duration>) -> Result<net::UdpSocket> {
    let mut failed_ports = HashSet::new();
    for _ in 0..100 {
        let port = rand::thread_rng().gen_range(0, 65_535);
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
