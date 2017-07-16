use std::{fmt, result, str, io};
use std::io::Write;
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};

#[derive(Debug)]
pub enum PacketErr {
    OverflowSize,
    InvalidOpCode,
    StrOutOfBounds,
    OpCodeOutOfBounds,
    ErrCodeOutOfBounds,
    Utf8Error(str::Utf8Error),
    IOError(io::Error),
}

impl From<str::Utf8Error> for PacketErr {
    fn from(err: str::Utf8Error) -> PacketErr {
        PacketErr::Utf8Error(err)
    }
}

impl From<io::Error> for PacketErr {
    fn from(err: io::Error) -> PacketErr {
        PacketErr::IOError(err)
    }
}

pub type Result<T> = result::Result<T, PacketErr>;

macro_rules! primitive_enum {
    (
        $( #[$enum_attr:meta] )*
        pub enum $enum_name:ident of $base_int:tt {
            $( $variant:ident = $value:expr, )+
        }
    ) => {
        $( #[$enum_attr] )*
        #[repr($base_int)]
        pub enum $enum_name {
            $( $variant = $value, )+
        }

        // TODO: change this to a From<u16> impl
        impl $enum_name {
            pub fn from_u16(i: $base_int) -> Result<$enum_name> {
                match i {
                    $( $value => Ok($enum_name::$variant), )+
                    _ => Err(PacketErr::OpCodeOutOfBounds)
                }
            }
        }
    }
}

primitive_enum! (
    #[derive(PartialEq, Clone, Debug)]
    pub enum OpCode of u16 {
        RRQ = 1,
        WRQ = 2,
        DATA = 3,
        ACK = 4,
        ERROR = 5,
    }
);

primitive_enum! (
    #[derive(PartialEq, Clone, Copy, Debug)]
    pub enum ErrorCode of u16 {
        NotDefined = 0,
        FileNotFound = 1,
        AccessViolation = 2,
        DiskFull = 3,
        IllegalTFTP = 4,
        UnknownID = 5,
        FileExists = 6,
        NoUser = 7,
    }
);

impl ErrorCode {
    /// Returns the string description of the error code.
    pub fn to_string(&self) -> String {
        (match *self {
             ErrorCode::NotDefined => "Not defined, see error message (if any).",
             ErrorCode::FileNotFound => "File not found.",
             ErrorCode::AccessViolation => "Access violation.",
             ErrorCode::DiskFull => "Disk full or allocation exceeded.",
             ErrorCode::IllegalTFTP => "Illegal TFTP operation.",
             ErrorCode::UnknownID => "Unknown transfer ID.",
             ErrorCode::FileExists => "File already exists.",
             ErrorCode::NoUser => "No such user.",
         }).to_string()
    }

    /// Returns the ERROR packet with the error code and
    /// the default description as the error message.
    pub fn to_packet(&self) -> Packet {
        let msg = self.to_string();
        Packet::ERROR {
            code: *self,
            msg: msg,
        }
    }
}

pub const MODES: [&'static str; 3] = ["netascii", "octet", "mail"];
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
        PacketData {
            bytes: bytes,
            len: len,
        }
    }

    /// Returns a byte slice that can be sent through a socket.
    pub fn to_slice<'a>(&'a self) -> &'a [u8] {
        &self.bytes[0..self.len]
    }
}

impl Clone for PacketData {
    fn clone(&self) -> PacketData {
        let mut bytes = [0; MAX_PACKET_SIZE];
        for i in 0..MAX_PACKET_SIZE {
            bytes[i] = self.bytes[i];
        }

        PacketData {
            bytes: bytes,
            len: self.len,
        }
    }
}

/// A wrapper around the data that is to be sent in a TFTP DATA packet
/// so that the data can be cloned and compared for equality.
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

#[derive(PartialEq, Clone, Debug)]
pub enum Packet {
    RRQ { filename: String, mode: String },
    WRQ { filename: String, mode: String },
    DATA {
        block_num: u16,
        data: DataBytes,
        len: usize,
    },
    ACK(u16),
    ERROR { code: ErrorCode, msg: String },
}

impl Packet {
    /// Creates and returns a packet parsed from its byte representation.
    pub fn read(pack_data: PacketData) -> Result<Packet> {
        let mut bytes = &pack_data.bytes[..pack_data.len];
        let opcode = OpCode::from_u16(bytes.read_u16::<BigEndian>()?)?;
        match opcode {
            OpCode::RRQ | OpCode::WRQ => read_rw_packet(opcode, &bytes),
            OpCode::DATA => read_data_packet(&bytes),
            OpCode::ACK => read_ack_packet(&bytes),
            OpCode::ERROR => read_error_packet(&bytes),
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
    pub fn to_bytes(self) -> Result<PacketData> {
        match self {
            Packet::RRQ { filename, mode } => rw_packet_bytes(OpCode::RRQ, filename, mode),
            Packet::WRQ { filename, mode } => rw_packet_bytes(OpCode::WRQ, filename, mode),
            Packet::DATA {
                block_num,
                data,
                len,
            } => data_packet_bytes(block_num, &data.0[..len]),
            Packet::ACK(block_num) => ack_packet_bytes(block_num),
            Packet::ERROR { code, msg } => error_packet_bytes(code, msg),
        }
    }
}

/// Reads until the zero byte and returns a string containing the bytes read
/// and the rest of the buffer, skipping the zero byte
fn read_string(bytes: &[u8]) -> Result<(String, &[u8])> {
    let result_bytes = bytes
        .iter()
        .take_while(|c| **c != 0)
        .cloned()
        .collect::<Vec<u8>>();
    // TODO: add test for error condition below
    if result_bytes.len() == bytes.len() {
        // reading didn't stop on a zero byte
        return Err(PacketErr::StrOutOfBounds);
    }

    let result_str = str::from_utf8(result_bytes.as_slice())?.to_string();
    let (_, tail) = bytes.split_at(result_bytes.len() + 1 /* +1 so we skip the \0 byte*/);
    Ok((result_str, tail))
}

fn read_rw_packet(code: OpCode, bytes: &[u8]) -> Result<Packet> {
    let (filename, rest) = read_string(&bytes)?;
    let (mode, _) = read_string(&rest)?;

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

fn read_data_packet(mut bytes: &[u8]) -> Result<Packet> {
    let block_num = bytes.read_u16::<BigEndian>()?;
    let mut data = [0; 512];
    // TODO: test with longer packets
    let len = (&mut data[..]).write(bytes)?;

    Ok(Packet::DATA {
        block_num: block_num,
        data: DataBytes(data),
        len: len,
    })
}

fn read_ack_packet(mut bytes: &[u8]) -> Result<Packet> {
    let block_num = bytes.read_u16::<BigEndian>()?;
    Ok(Packet::ACK(block_num))
}

fn read_error_packet(mut bytes: &[u8]) -> Result<Packet> {
    let error_code = ErrorCode::from_u16(bytes.read_u16::<BigEndian>()?)?;
    let (msg, _) = read_string(&bytes)?;

    Ok(Packet::ERROR {
        code: error_code,
        msg: msg,
    })
}

fn rw_packet_bytes(packet: OpCode, filename: String, mode: String) -> Result<PacketData> {
    if filename.len() + mode.len() > MAX_PACKET_SIZE {
        return Err(PacketErr::OverflowSize);
    }

    let mut bytes = [0; MAX_PACKET_SIZE];

    let leftover = {
        let mut buf = &mut bytes[..];

        buf.write_u16::<BigEndian>(packet as u16)?;
        buf.write_all(filename.as_bytes())?;
        buf.write_all(&[0])?;
        buf.write_all(mode.as_bytes())?;
        buf.write_all(&[0])?;

        buf.len() - 1 // TODO: figure out why this is needed
    };

    Ok(PacketData::new(bytes, bytes[..].len() - leftover))
}

fn data_packet_bytes(block_num: u16, data: &[u8]) -> Result<PacketData> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    let leftover = {
        let mut buf = &mut bytes[..];

        buf.write_u16::<BigEndian>(OpCode::DATA as u16)?;
        buf.write_u16::<BigEndian>(block_num)?;
        buf.write_all(data)?;

        buf.len()
    };

    Ok(PacketData::new(bytes, bytes[..].len() - leftover))
}

fn ack_packet_bytes(block_num: u16) -> Result<PacketData> {
    let mut bytes = [0; MAX_PACKET_SIZE];

    {
        let mut buf = &mut bytes[..];

        buf.write_u16::<BigEndian>(OpCode::ACK as u16)?;
        buf.write_u16::<BigEndian>(block_num)?;
    }

    Ok(PacketData::new(bytes, 4))
}

fn error_packet_bytes(code: ErrorCode, msg: String) -> Result<PacketData> {
    if msg.len() + 5 > MAX_PACKET_SIZE {
        return Err(PacketErr::OverflowSize);
    }

    let mut bytes = [0; MAX_PACKET_SIZE];

    let leftover = {
        let mut buf = &mut bytes[..];

        buf.write_u16::<BigEndian>(OpCode::ERROR as u16)?;
        buf.write_u16::<BigEndian>(code as u16)?;
        buf.write_all(msg.as_bytes())?;
        buf.write_all(&[0])?;

        buf.len()
    };

    Ok(PacketData::new(bytes, bytes[..].len() - leftover))
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

            let result = read_string(&bytes[$start_pos..seed_bytes.len()]);
            assert!(result.is_ok());
            let _ = result.map(|(string, rest)| {
                assert_eq!(string, $string);
                assert_eq!(seed_bytes.len() - rest.len(), $end_pos);
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
