use std::{result, str, io};
use std::io::Write;
use byteorder::{ReadBytesExt, WriteBytesExt, BigEndian};
use read_512::Read512;

#[derive(Debug)]
pub enum PacketErr {
    StrOutOfBounds,
    OpCodeOutOfBounds,
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
            fn from_u16(i: $base_int) -> Result<$enum_name> {
                match i {
                    $( $value => Ok($enum_name::$variant), )+
                    _ => Err(PacketErr::OpCodeOutOfBounds)
                }
            }
        }
    }
}

primitive_enum! (
    #[derive(PartialEq, Copy, Clone, Debug)]
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
}

impl From<ErrorCode> for Packet {
    /// Returns the ERROR packet with the error code and
    /// the default description as the error message.
    fn from(code: ErrorCode) -> Packet {
        let msg = code.to_string();
        Packet::ERROR {
            code,
            msg,
        }
    }
}

pub const MAX_PACKET_SIZE: usize = 1024;

/// The byte representation of a packet
pub struct PacketData(Vec<u8>);

impl PacketData {
    /// Returns a byte slice that can be sent through a socket.
    pub fn to_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub enum Packet {
    RRQ { filename: String, mode: String },
    WRQ { filename: String, mode: String },
    DATA { block_num: u16, data: Vec<u8> },
    ACK(u16),
    ERROR { code: ErrorCode, msg: String },
}

impl Packet {
    /// Creates and returns a packet parsed from its byte representation.
    pub fn read(mut bytes: &[u8]) -> Result<Packet> {
        let opcode = OpCode::from_u16(bytes.read_u16::<BigEndian>()?)?;
        match opcode {
            OpCode::RRQ => read_rrq_packet(bytes),
            OpCode::WRQ => read_wrq_packet(bytes),
            OpCode::DATA => read_data_packet(bytes),
            OpCode::ACK => read_ack_packet(bytes),
            OpCode::ERROR => read_error_packet(bytes),
        }
    }

    /// Consumes the packet and returns the packet in byte representation.
    pub fn into_bytes(self) -> Result<PacketData> {
        self.to_bytes()
    }

    /// Returns the byte representation of the packet
    pub fn to_bytes(&self) -> Result<PacketData> {
        match *self {
            Packet::RRQ {
                ref filename,
                ref mode,
            } => rw_packet_bytes(OpCode::RRQ, filename, mode),
            Packet::WRQ {
                ref filename,
                ref mode,
            } => rw_packet_bytes(OpCode::WRQ, filename, mode),
            Packet::DATA {
                block_num,
                ref data,
            } => data_packet_bytes(block_num, data.as_slice()),
            Packet::ACK(block_num) => ack_packet_bytes(block_num),
            Packet::ERROR { code, ref msg } => error_packet_bytes(code, msg),
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

fn read_rrq_packet(bytes: &[u8]) -> Result<Packet> {
    let (filename, rest) = read_string(bytes)?;
    let (mode, _) = read_string(rest)?;

    Ok(Packet::RRQ {
        filename,
        mode,
    })
}

fn read_wrq_packet(bytes: &[u8]) -> Result<Packet> {
    let (filename, rest) = read_string(bytes)?;
    let (mode, _) = read_string(rest)?;

    Ok(Packet::WRQ {
        filename,
        mode,
    })
}

fn read_data_packet(mut bytes: &[u8]) -> Result<Packet> {
    let block_num = bytes.read_u16::<BigEndian>()?;
    let mut data = Vec::with_capacity(512);
    // TODO: test with longer packets
    bytes.read_512(&mut data)?;

    Ok(Packet::DATA {
        block_num,
        data,
    })
}

fn read_ack_packet(mut bytes: &[u8]) -> Result<Packet> {
    let block_num = bytes.read_u16::<BigEndian>()?;
    Ok(Packet::ACK(block_num))
}

fn read_error_packet(mut bytes: &[u8]) -> Result<Packet> {
    let code = ErrorCode::from_u16(bytes.read_u16::<BigEndian>()?)?;
    let (msg, _) = read_string(bytes)?;

    Ok(Packet::ERROR {
        code,
        msg,
    })
}

fn rw_packet_bytes(packet: OpCode, filename: &str, mode: &str) -> Result<PacketData> {
    let mut buf = Vec::with_capacity(MAX_PACKET_SIZE);

    buf.write_u16::<BigEndian>(packet as u16)?;
    buf.write_all(filename.as_bytes())?;
    buf.push(0);
    buf.write_all(mode.as_bytes())?;
    buf.push(0);

    Ok(PacketData(buf))
}

fn data_packet_bytes(block_num: u16, data: &[u8]) -> Result<PacketData> {
    let mut buf = Vec::with_capacity(MAX_PACKET_SIZE);

    buf.write_u16::<BigEndian>(OpCode::DATA as u16)?;
    buf.write_u16::<BigEndian>(block_num)?;
    buf.write_all(data)?;

    Ok(PacketData(buf))
}

fn ack_packet_bytes(block_num: u16) -> Result<PacketData> {
    let mut buf = Vec::with_capacity(MAX_PACKET_SIZE);

    buf.write_u16::<BigEndian>(OpCode::ACK as u16)?;
    buf.write_u16::<BigEndian>(block_num)?;

    Ok(PacketData(buf))
}

fn error_packet_bytes(code: ErrorCode, msg: &str) -> Result<PacketData> {
    let mut buf = Vec::with_capacity(MAX_PACKET_SIZE);

    buf.write_u16::<BigEndian>(OpCode::ERROR as u16)?;
    buf.write_u16::<BigEndian>(code as u16)?;
    buf.write_all(msg.as_bytes())?;
    buf.push(0);

    Ok(PacketData(buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    macro_rules! test_read_string {
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

    test_read_string!(
        test_read_string_normal,
        "hello world!\0",
        0,
        "hello world!",
        13
    );
    test_read_string!(
        test_read_string_zero_in_mid,
        "hello wor\0ld!",
        0,
        "hello wor",
        10
    );
    test_read_string!(
        test_read_string_diff_start_pos,
        "hello world!\0",
        6,
        "world!",
        13
    );

    macro_rules! packet_enc_dec_test {
        ($name:ident, $packet:expr) => {
            #[test]
            fn $name() {
                let bytes = $packet.clone().into_bytes();
                assert!(bytes.is_ok());
                let packet = bytes.and_then(|pd| Packet::read(pd.to_slice()));
                assert!(packet.is_ok());
                let _ = packet.map(|packet| { assert_eq!(packet, $packet); });
            }
        };
    }

    const BYTE_DATA: [u8; 512] = [123; 512];

    packet_enc_dec_test!(
        rrq,
        Packet::RRQ {
            filename: "/a/b/c/hello.txt".to_string(),
            mode: "netascii".to_string(),
        }
    );
    packet_enc_dec_test!(
        wrq,
        Packet::WRQ {
            filename: "./world.txt".to_string(),
            mode: "octet".to_string(),
        }
    );
    packet_enc_dec_test!(ack, Packet::ACK(1234));
    packet_enc_dec_test!(
        data,
        Packet::DATA {
            block_num: 1234,
            data: Vec::from(&BYTE_DATA[..]),
        }
    );
    packet_enc_dec_test!(
        err,
        Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "This is a message".to_string(),
        }
    );
}
