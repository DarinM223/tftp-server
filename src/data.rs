use std::{mem, result};
use std::io;

#[repr(u16)]
pub enum OpCode {
    RRQ = 1,
    WRQ = 2,
    DATA = 3,
    ACK = 4,
    ERROR = 5,
}

impl OpCode {
    pub fn from_u16(i: u16) -> OpCode {
        assert!(i >= OpCode::RRQ as u16 && i <= OpCode::ERROR as u16);
        unsafe { mem::transmute(i) }
    }
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
pub const MAX_DATA_SIZE: usize = 516;

pub enum Packet {
    RRQ {
        filename: String,
        mode: &'static str,
    },
    WRQ {
        filename: String,
        mode: &'static str,
    },
    DATA {
        block_num: u16,
        data: [u8; 512],
    },
    ACK(u16),
    ERROR {
        code: u16,
        msg: String,
    },
}

pub type PacketData = [u8; MAX_PACKET_SIZE];
pub type Result<T> = result::Result<T, PacketErr>;

pub enum PacketErr {
    OverflowSize,
}

impl PacketErr {
    pub fn str(&self) -> &'static str {
        match *self {
            PacketErr::OverflowSize => "Packet size has overflowed",
        }
    }
}

impl From<PacketErr> for io::Error {
    fn from(err: PacketErr) -> io::Error {
        io::Error::new(io::ErrorKind::Other, err.str())
    }
}

impl Packet {
    pub fn read(bytes: PacketData) -> Result<Packet> {
        let opcode = OpCode::from_u16(merge_bytes(bytes[0], bytes[1]));
        match opcode {
            OpCode::RRQ | OpCode::WRQ => read_rw_packet(opcode, bytes),
            OpCode::DATA => read_data_packet(bytes),
            OpCode::ACK => read_ack_packet(bytes),
            OpCode::ERROR => read_error_packet(bytes),
        }
    }

    /// Returns the packet's operation code.
    pub fn op_code(&self) -> OpCode {
        match *self {
            Packet::RRQ { .. } => OpCode::RRQ,
            Packet::WRQ { .. } => OpCode::WRQ,
            Packet::DATA { .. } => OpCode::DATA,
            Packet::ACK(_) => OpCode::ACK,
            Packet::ERROR { .. } => OpCode::ERROR,
        }
    }

    /// Consumes the packet and returns the packet in byte representation.
    pub fn bytes(self) -> Result<PacketData> {
        match self {
            Packet::RRQ { filename, mode } => rw_packet_bytes(OpCode::RRQ, filename, mode),
            Packet::WRQ { filename, mode } => rw_packet_bytes(OpCode::WRQ, filename, mode),
            Packet::DATA { block_num, data } => data_packet_bytes(block_num, data),
            Packet::ACK(block_num) => ack_packet_bytes(block_num),
            Packet::ERROR { code, msg } => error_packet_bytes(code, msg),
        }
    }
}

/// Splits a two byte unsigned integer into two one byte unsigned integers.
pub fn split_into_bytes(num: u16) -> (u8, u8) {
    let (b0, b1) = (num & 0xFF, (num >> 8) & (0xFF));
    (b0 as u8, b1 as u8)
}

pub fn merge_bytes(num1: u8, num2: u8) -> u16 {
    unimplemented!()
}

fn read_rw_packet(code: OpCode, bytes: PacketData) -> Result<Packet> {
    unimplemented!()
}

fn read_data_packet(bytes: PacketData) -> Result<Packet> {
    unimplemented!()
}

fn read_ack_packet(bytes: PacketData) -> Result<Packet> {
    unimplemented!()
}

fn read_error_packet(bytes: PacketData) -> Result<Packet> {
    unimplemented!()
}

fn rw_packet_bytes(packet: OpCode, filename: String, mode: &'static str) -> Result<PacketData> {
    if filename.len() + mode.len() > MAX_PACKET_SIZE {
        return Err(PacketErr::OverflowSize);
    }

    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(packet as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let mut index = 2;
    for byte in filename.bytes() {
        bytes[index] = byte;
        index += 1;
    }

    index += 1;
    for byte in mode.bytes() {
        bytes[index] = byte;
        index += 1;
    }

    Ok(bytes)
}

fn data_packet_bytes(block_num: u16, data: [u8; 512]) -> Result<PacketData> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::DATA as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(block_num);
    bytes[2] = b3;
    bytes[3] = b4;

    let mut index = 4;
    for byte in data.into_iter() {
        bytes[index] = *byte;
        index += 1;
    }

    Ok(bytes)
}

fn ack_packet_bytes(block_num: u16) -> Result<PacketData> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::ACK as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(block_num);
    bytes[2] = b3;
    bytes[3] = b4;

    Ok(bytes)
}

fn error_packet_bytes(code: u16, msg: String) -> Result<PacketData> {
    if msg.len() + 5 > MAX_PACKET_SIZE {
        return Err(PacketErr::OverflowSize);
    }

    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::ERROR as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(code);
    bytes[2] = b3;
    bytes[3] = b4;

    let mut index = 4;
    for byte in msg.bytes() {
        bytes[index] = byte;
        index += 1;
    }

    Ok(bytes)
}
