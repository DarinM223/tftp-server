use std::{fmt, io, mem, result, str};
use std::marker::PhantomData;

#[repr(u16)]
#[derive(PartialEq, Clone, Debug)]
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
#[derive(PartialEq, Clone, Debug)]
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
    pub fn from_u16(i: u16) -> ErrorCode {
        assert!(i >= ErrorCode::NotDefined as u16 && i <= ErrorCode::NoUser as u16);
        unsafe { mem::transmute(i) }
    }
}

pub const MODES: [&'static str; 3] = ["netascii", "octet", "mail"];
pub const MAX_PACKET_SIZE: usize = 1024;
pub const MAX_DATA_SIZE: usize = 516;

pub struct PacketData<'a> {
    bytes: [u8; MAX_PACKET_SIZE],
    len: usize,
    phantom: PhantomData<&'a i32>,
}

impl<'a> PacketData<'a> {
    pub fn new(bytes: [u8; MAX_PACKET_SIZE], len: usize) -> PacketData<'a> {
        PacketData {
            bytes: bytes,
            len: len,
            phantom: PhantomData,
        }
    }

    pub fn to_slice(&'a self) -> &'a [u8] {
        &self.bytes[0..self.len]
    }
}

impl<'a> Clone for PacketData<'a> {
    fn clone(&self) -> PacketData<'a> {
        let mut bytes = [0; MAX_PACKET_SIZE];
        for i in 0..MAX_PACKET_SIZE {
            bytes[i] = self.bytes[i];
        }

        PacketData {
            bytes: bytes,
            len: self.len,
            phantom: self.phantom,
        }
    }
}

pub type Result<T> = result::Result<T, PacketErr>;

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

impl Clone for DataBytes {
    fn clone(&self) -> DataBytes {
        let mut bytes = [0; 512];
        for i in 0..512 {
            bytes[i] = self.0[i];
        }

        DataBytes(bytes)
    }
}

impl fmt::Debug for DataBytes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0[..]))
    }
}

#[derive(Debug)]
pub enum PacketErr {
    OverflowSize,
    InvalidOpCode,
    StrOutOfBounds,
    Utf8Error(usize),
}

impl PacketErr {
    pub fn to_string(&self) -> String {
        match *self {
            PacketErr::OverflowSize => "Packet size has overflowed".to_string(),
            PacketErr::InvalidOpCode => "Packet opcode is invalid".to_string(),
            PacketErr::StrOutOfBounds => {
                "A string in the packet is not zero byte terminated".to_string()
            }
            PacketErr::Utf8Error(up_to) => format!("UTF8 string only valid up to {}", up_to),
        }
    }
}

impl From<PacketErr> for io::Error {
    fn from(err: PacketErr) -> io::Error {
        io::Error::new(io::ErrorKind::Other, err.to_string())
    }
}

impl From<str::Utf8Error> for PacketErr {
    fn from(err: str::Utf8Error) -> PacketErr {
        PacketErr::Utf8Error(err.valid_up_to())
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
        let opcode = OpCode::from_u16(merge_bytes(bytes.bytes[0], bytes.bytes[1]));
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
    pub fn bytes<'a>(self) -> Result<PacketData<'a>> {
        match self {
            Packet::RRQ { filename, mode } => rw_packet_bytes(OpCode::RRQ, filename, mode),
            Packet::WRQ { filename, mode } => rw_packet_bytes(OpCode::WRQ, filename, mode),
            Packet::DATA { block_num, data, len } => data_packet_bytes(block_num, data.0, len),
            Packet::ACK(block_num) => ack_packet_bytes(block_num),
            Packet::ERROR { code, msg } => error_packet_bytes(code, msg),
        }
    }
}

/// Splits a two byte unsigned integer into two one byte unsigned integers.
fn split_into_bytes(num: u16) -> (u8, u8) {
    let (b0, b1) = (num & 0xFF, (num >> 8) & (0xFF));
    (b0 as u8, b1 as u8)
}

/// Merges two 1 byte unsigned integers into a two byte unsigned integer.
fn merge_bytes(num1: u8, num2: u8) -> u16 {
    (num1 as u16) + ((num2 as u16) << 8)
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

    let result_str = try!(str::from_utf8(result_bytes.as_slice())).to_string();
    Ok((result_str, counter))
}

fn read_rw_packet(code: OpCode, bytes: PacketData) -> Result<Packet> {
    let (filename, end_pos) = try!(read_string(&bytes, 2));
    let (mode, _) = try!(read_string(&bytes, end_pos));

    match code {
        OpCode::RRQ => {
            Ok(Packet::RRQ {
                filename: filename,
                mode: mode,
            })
        }
        OpCode::WRQ => {
            Ok(Packet::WRQ {
                filename: filename,
                mode: mode,
            })
        }
        _ => Err(PacketErr::InvalidOpCode),
    }
}

fn read_data_packet(bytes: PacketData) -> Result<Packet> {
    let block_num = merge_bytes(bytes.bytes[2], bytes.bytes[3]);
    let mut data = [0; 512];
    // TODO(DarinM223): refactor to use iterators instead of indexing
    for i in 0..512 {
        data[i] = bytes.bytes[i + 4];
    }

    Ok(Packet::DATA {
        block_num: block_num,
        data: DataBytes(data),
        len: bytes.len - 4,
    })
}

fn read_ack_packet(bytes: PacketData) -> Result<Packet> {
    let block_num = merge_bytes(bytes.bytes[2], bytes.bytes[3]);
    Ok(Packet::ACK(block_num))
}

fn read_error_packet(bytes: PacketData) -> Result<Packet> {
    let error_code = ErrorCode::from_u16(merge_bytes(bytes.bytes[2], bytes.bytes[3]));
    let (msg, _) = try!(read_string(&bytes, 4));

    Ok(Packet::ERROR {
        code: error_code,
        msg: msg,
    })
}

fn rw_packet_bytes<'a>(packet: OpCode, filename: String, mode: String) -> Result<PacketData<'a>> {
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

fn data_packet_bytes<'a>(block_num: u16,
                         data: [u8; 512],
                         data_len: usize)
                         -> Result<PacketData<'a>> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::DATA as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(block_num);
    bytes[2] = b3;
    bytes[3] = b4;

    let mut index = 4;
    for i in 0..data_len {
        bytes[index] = data[i];
        index += 1;
    }

    Ok(PacketData::new(bytes, index))
}

fn ack_packet_bytes<'a>(block_num: u16) -> Result<PacketData<'a>> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let (b1, b2) = split_into_bytes(OpCode::ACK as u16);
    bytes[0] = b1;
    bytes[1] = b2;

    let (b3, b4) = split_into_bytes(block_num);
    bytes[2] = b3;
    bytes[3] = b4;

    Ok(PacketData::new(bytes, 4))
}

fn error_packet_bytes<'a>(code: ErrorCode, msg: String) -> Result<PacketData<'a>> {
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

#[test]
fn test_split_merge_bytes() {
    let tests = [0, 1234, 9876];
    for test in tests.iter() {
        let (b1, b2) = split_into_bytes(*test);
        let merged = merge_bytes(b1, b2);

        assert_eq!(*test, merged);
    }
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

read_string!(test_read_string_normal,
             "hello world!\0",
             0,
             "hello world!",
             13);
read_string!(test_read_string_zero_in_mid,
             "hello wor\0ld!",
             0,
             "hello wor",
             10);
read_string!(test_read_string_diff_start_pos,
             "hello world!\0",
             6,
             "world!",
             13);
