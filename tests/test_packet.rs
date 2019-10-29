extern crate tftp_server;

use tftp_server::packet::*;

macro_rules! packet {
    ($name:ident, $packet:expr) => {
        #[test]
        fn $name() {
            let bytes = $packet.clone().bytes();
            assert!(bytes.is_ok());
            let packet = bytes.and_then(|bytes| Packet::read(bytes));
            assert!(packet.is_ok());
            let _ = packet.map(|packet| {
                assert_eq!(packet, $packet);
            });
        }
    };
}

const BYTE_DATA: [u8; 512] = [123; 512];

packet!(
    rrq,
    Packet::RRQ {
        filename: "/a/b/c/hello.txt".to_string(),
        mode: "netascii".to_string(),
    }
);
packet!(
    wrq,
    Packet::WRQ {
        filename: "./world.txt".to_string(),
        mode: "octet".to_string(),
    }
);
packet!(ack, Packet::ACK(1234));
packet!(
    data,
    Packet::DATA {
        block_num: 1234,
        data: DataBytes(BYTE_DATA),
        len: 512,
    }
);
packet!(
    err,
    Packet::ERROR {
        code: ErrorCode::NoUser,
        msg: "This is a message".to_string(),
    }
);
