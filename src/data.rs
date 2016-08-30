use std::io;

#[repr(u16)]
pub enum OpCode {
    RRQ = 1,
    WRQ = 2,
    DATA = 3,
    ACK = 4,
    ERROR = 5,
}

#[repr(u16)]
pub enum ErrorCode {
    NotDefined = 0,
    FileNotFound = 1,
    AccessViolation = 2,
    DiskFull = 3,
    IllegalTFTP = 4,
    UnknownID = 5,
    FileExists = 6,
    NoUser = 7,
}

pub const MODES: [&'static str; 3] = ["netascii", "octet", "mail"];
pub const MAX_PACKET_SIZE: usize = 1024;

pub enum Packet {
    RRQ { filename: String, mode: &'static str },
    WRQ { filename: String, mode: &'static str },
    DATA { block_num: u16, data: [u8; 512] },
    ACK(u16),
    ERROR { code: u16, msg: String },
}

impl Packet {
    pub fn op_code(&self) -> OpCode {
        match *self {
            Packet::RRQ { .. } => OpCode::RRQ,
            Packet::WRQ { .. } => OpCode::WRQ,
            Packet::DATA { .. } => OpCode::DATA,
            Packet::ACK(_) => OpCode::ACK,
            Packet::ERROR { .. } => OpCode::ERROR,
        }
    }

    pub fn bytes<'a>(self) -> io::Result<([u8; MAX_PACKET_SIZE], usize)> {
        match self {
            Packet::RRQ { filename, mode } => rrq_packet_bytes(filename, mode),
            Packet::WRQ { filename, mode } => wrq_packet_bytes(filename, mode),
            Packet::DATA { block_num, data } => data_packet_bytes(block_num, data),
            Packet::ACK(block_num) => ack_packet_bytes(block_num),
            Packet::ERROR { code, msg } => error_packet_bytes(code, msg),
        }
    }
}

fn rrq_packet_bytes(filename: String, mode: &'static str) -> io::Result<([u8; MAX_PACKET_SIZE], usize)> {
    unimplemented!()
}

fn wrq_packet_bytes(filename: String, mode: &'static str) -> io::Result<([u8; MAX_PACKET_SIZE], usize)> {
    unimplemented!()
}

fn data_packet_bytes(block_num: u16, data: [u8; 512]) -> io::Result<([u8; MAX_PACKET_SIZE], usize)> {
    unimplemented!()
}

fn ack_packet_bytes<'a>(block_num: u16) -> io::Result<([u8; MAX_PACKET_SIZE], usize)> {
    unimplemented!()
}

fn error_packet_bytes<'a>(code: u16, msg: String) -> io::Result<([u8; MAX_PACKET_SIZE], usize)> {
    unimplemented!()
}
