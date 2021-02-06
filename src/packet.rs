use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use num_derive::FromPrimitive;
use std::io::Cursor;
use std::{fmt, result, str};

#[derive(Debug, PartialEq)]
pub enum PacketErr {
    OverflowSize,
    InvalidOpCode,
    StrOutOfBounds,
    OpCodeOutOfBounds,
    ErrCodeOutOfBounds,
    Utf8Error(str::Utf8Error),
}

impl From<str::Utf8Error> for PacketErr {
    fn from(err: str::Utf8Error) -> PacketErr {
        PacketErr::Utf8Error(err)
    }
}

pub type Result<T> = result::Result<T, PacketErr>;

#[repr(u16)]
#[derive(PartialEq, Clone, Debug, FromPrimitive)]
pub enum OpCode {
    RRQ = 1,
    WRQ = 2,
    DATA = 3,
    ACK = 4,
    ERROR = 5,
}

impl OpCode {
    pub fn from_u16(i: u16) -> Result<OpCode> {
        num_traits::FromPrimitive::from_u16(i).ok_or(PacketErr::OpCodeOutOfBounds)
    }
}

#[repr(u16)]
#[derive(PartialEq, Clone, Copy, Debug, FromPrimitive)]
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

impl ErrorCode {
    pub fn from_u16(i: u16) -> Result<ErrorCode> {
        num_traits::FromPrimitive::from_u16(i).ok_or(PacketErr::ErrCodeOutOfBounds)
    }

    /// Returns the ERROR packet with the error code and
    /// the default description as the error message.
    pub fn to_packet(self) -> Packet {
        Packet::ERROR {
            code: self,
            msg: self.to_string(),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match *self {
            ErrorCode::NotDefined => "Not defined, see error message (if any).",
            ErrorCode::FileNotFound => "File not found.",
            ErrorCode::AccessViolation => "Access violation.",
            ErrorCode::DiskFull => "Disk full or allocation exceeded.",
            ErrorCode::IllegalTFTP => "Illegal TFTP operation.",
            ErrorCode::UnknownID => "Unknown transfer ID.",
            ErrorCode::FileExists => "File already exists.",
            ErrorCode::NoUser => "No such user.",
        };
        write!(f, "{}", s)
    }
}

pub const MODES: [&str; 3] = ["netascii", "octet", "mail"];
pub const MAX_PACKET_SIZE: usize = 1024;
pub const MAX_DATA_SIZE: usize = 516;

/// The byte representation of a packet. Because many packets can
/// be smaller than the maximum packet size, it contains a length
/// parameter so that the actual packet size can be determined.
pub struct PacketData {
    bytes: [u8; MAX_PACKET_SIZE],
    len: usize,
}

impl PacketData {
    pub fn new(bytes: [u8; MAX_PACKET_SIZE], len: usize) -> PacketData {
        PacketData { bytes, len }
    }

    /// Returns a byte slice that can be sent through a socket.
    pub fn to_slice(&self) -> &[u8] {
        &self.bytes[0..self.len]
    }
}

impl Clone for PacketData {
    fn clone(&self) -> PacketData {
        PacketData {
            bytes: self.bytes,
            len: self.len,
        }
    }
}

/// A wrapper around the data that is to be sent in a TFTP DATA packet
/// so that the data can be cloned and compared for equality.
#[derive(Clone)]
pub struct DataBytes(pub [u8; 512]);

impl PartialEq for DataBytes {
    fn eq(&self, other: &DataBytes) -> bool {
        for i in 0..512 {
            if self.0[i] != other.0[i] {
                return false;
            }
        }

        true
    }
}

impl fmt::Debug for DataBytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0[..]))
    }
}

#[derive(PartialEq, Clone, Debug)]
pub enum Packet {
    RRQ {
        filename: String,
        mode: String,
    },
    WRQ {
        filename: String,
        mode: String,
    },
    DATA {
        block_num: u16,
        data: DataBytes,
        len: usize,
    },
    ACK(u16),
    ERROR {
        code: ErrorCode,
        msg: String,
    },
}

impl Packet {
    /// Creates and returns a packet parsed from its byte representation.
    pub fn read(bytes: PacketData) -> Result<Packet> {
        let opcode = OpCode::from_u16(merge_bytes(bytes.bytes[0], bytes.bytes[1]))?;
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
            Packet::DATA {
                block_num,
                data,
                len,
            } => data_packet_bytes(block_num, data.0, len),
            Packet::ACK(block_num) => ack_packet_bytes(block_num),
            Packet::ERROR { code, msg } => error_packet_bytes(code, msg),
        }
    }
}

/// Splits a two byte unsigned integer into two one byte unsigned integers.
fn split_into_bytes(num: u16) -> (u8, u8) {
    let mut wtr = vec![];
    wtr.write_u16::<BigEndian>(num).unwrap();

    (wtr[0], wtr[1])
}

/// Merges two 1 byte unsigned integers into a two byte unsigned integer.
fn merge_bytes(num1: u8, num2: u8) -> u16 {
    let mut rdr = Cursor::new(vec![num1, num2]);
    rdr.read_u16::<BigEndian>().unwrap()
}

/// Reads bytes from the packet bytes starting from the given index
/// until the zero byte and returns a string containing the bytes read.
fn read_string(bytes: &PacketData, start: usize) -> Result<(String, usize)> {
    let mut result_bytes = Vec::new();
    let mut counter = start;
    while bytes.bytes[counter] != 0 {
        result_bytes.push(bytes.bytes[counter]);

        counter += 1;
        if counter >= bytes.len {
            return Err(PacketErr::StrOutOfBounds);
        }
    }
    counter += 1;

    let result_str = str::from_utf8(result_bytes.as_slice())?.to_string();
    Ok((result_str, counter))
}

fn read_rw_packet(code: OpCode, bytes: PacketData) -> Result<Packet> {
    let (filename, end_pos) = read_string(&bytes, 2)?;
    let (mode, _) = read_string(&bytes, end_pos)?;

    match code {
        OpCode::RRQ => Ok(Packet::RRQ { filename, mode }),
        OpCode::WRQ => Ok(Packet::WRQ { filename, mode }),
        _ => Err(PacketErr::InvalidOpCode),
    }
}

fn read_data_packet(bytes: PacketData) -> Result<Packet> {
    let block_num = merge_bytes(bytes.bytes[2], bytes.bytes[3]);
    let mut data = [0; 512];
    data[0..512].clone_from_slice(&bytes.bytes[4..(512 + 4)]);

    Ok(Packet::DATA {
        block_num,
        data: DataBytes(data),
        len: bytes.len - 4,
    })
}

fn read_ack_packet(bytes: PacketData) -> Result<Packet> {
    let block_num = merge_bytes(bytes.bytes[2], bytes.bytes[3]);
    Ok(Packet::ACK(block_num))
}

fn read_error_packet(bytes: PacketData) -> Result<Packet> {
    let error_code = ErrorCode::from_u16(merge_bytes(bytes.bytes[2], bytes.bytes[3]))?;
    let (msg, _) = read_string(&bytes, 4)?;

    Ok(Packet::ERROR {
        code: error_code,
        msg,
    })
}

fn rw_packet_bytes(packet: OpCode, filename: String, mode: String) -> Result<PacketData> {
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
    index += 1;

    Ok(PacketData::new(bytes, index))
}

fn data_packet_bytes(block_num: u16, data: [u8; 512], data_len: usize) -> Result<PacketData> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::DATA as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(block_num);
    bytes[2] = b3;
    bytes[3] = b4;

    bytes[4..(data_len + 4)].clone_from_slice(&data[0..data_len]);

    Ok(PacketData::new(bytes, data_len + 4))
}

fn ack_packet_bytes(block_num: u16) -> Result<PacketData> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::ACK as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(block_num);
    bytes[2] = b3;
    bytes[3] = b4;

    Ok(PacketData::new(bytes, 4))
}

fn error_packet_bytes(code: ErrorCode, msg: String) -> Result<PacketData> {
    if msg.len() + 5 > MAX_PACKET_SIZE {
        return Err(PacketErr::OverflowSize);
    }

    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::ERROR as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(code as u16);
    bytes[2] = b3;
    bytes[3] = b4;

    let mut index = 4;
    for byte in msg.bytes() {
        bytes[index] = byte;
        index += 1;
    }
    index += 1;

    Ok(PacketData::new(bytes, index))
}

macro_rules! read_string {
    ($name:ident, $bytes:expr, $start_pos:expr, $string:expr, $end_pos:expr) => {
        #[test]
        fn $name() {
            let mut bytes = [0; MAX_PACKET_SIZE];
            let seed_bytes = $bytes.chars().collect::<Vec<_>>();
            for i in 0..$bytes.len() {
                bytes[i] = seed_bytes[i] as u8;
            }

            let result = read_string(&PacketData::new(bytes, seed_bytes.len()), $start_pos);
            assert!(result.is_ok());
            let _ = result.map(|(string, end_pos)| {
                assert_eq!(string, $string);
                assert_eq!(end_pos, $end_pos);
            });
        }
    };
}

read_string!(
    test_read_string_normal,
    "hello world!\0",
    0,
    "hello world!",
    13
);
read_string!(
    test_read_string_zero_in_mid,
    "hello wor\0ld!",
    0,
    "hello wor",
    10
);
read_string!(
    test_read_string_diff_start_pos,
    "hello world!\0",
    6,
    "world!",
    13
);

#[cfg(test)]
mod from_primitive_tests {
    use super::*;

    #[test]
    fn test_encode_opcode() {
        assert_eq!(OpCode::from_u16(0), Err(PacketErr::OpCodeOutOfBounds));
        assert_eq!(OpCode::from_u16(1), Ok(OpCode::RRQ));
        assert_eq!(OpCode::from_u16(5), Ok(OpCode::ERROR));
        assert_eq!(OpCode::from_u16(6), Err(PacketErr::OpCodeOutOfBounds));
    }

    #[test]
    fn test_encode_errorcode() {
        assert_eq!(ErrorCode::from_u16(0), Ok(ErrorCode::NotDefined));
        assert_eq!(ErrorCode::from_u16(7), Ok(ErrorCode::NoUser));
        assert_eq!(ErrorCode::from_u16(8), Err(PacketErr::ErrCodeOutOfBounds));
    }
}
