use std::io::Write;
use packet::{ErrorCode, Packet};
use server::{IOAdapter, Read512};

#[derive(Debug, PartialEq)]
pub enum TftpResult {
    /// Indicates the packet should be sent back to the client,
    /// and the transfer may continue
    Reply(Packet),

    /// Signals the calling code that it should resend the last packet
    Repeat,

    /// Indicates that the packet (if any) should be sent back to the client,
    /// and the transfer is considered terminated
    Done(Option<Packet>),

    /// Indicates an error encountered while processing the packet
    Err(TftpError),
}

#[derive(Debug, PartialEq)]
pub enum TftpError {
    /// The is already running and cannot be restarted
    TransferAlreadyRunning,

    /// The received packet type cannot be used to initiate a transfer
    NotIniatingPacket,
}

// TODO: this needs to be redone as a 2-variant enum (probably)
pub struct Transfer<IO: IOAdapter> {
    fread: Option<IO::R>,
    expected_block_num: u16,
    sent_final: bool,
    fwrite: Option<IO::W>,
    complete: bool,
}

/// The TFTP protocol and filesystem usage implementation,
/// used as backend for a TFTP server
pub struct TftpServerProto<IO: IOAdapter> {
    io: IO,
}

impl<IO: IOAdapter> TftpServerProto<IO> {
    /// Creates a new instance with the provided IOAdapter
    pub(crate) fn new(io: IO) -> Self {
        TftpServerProto { io: io }
    }

    pub(crate) fn rx_initial(&mut self, packet: Packet) -> (Option<Transfer<IO>>, TftpResult) {
        match packet {
            Packet::RRQ { filename, mode } => self.handle_rrq(&filename, &mode),
            Packet::WRQ { filename, mode } => self.handle_wrq(&filename, &mode),
            _ => (None, TftpResult::Err(TftpError::NotIniatingPacket)),
        }
    }

    fn handle_wrq(&mut self, filename: &str, mode: &str) -> (Option<Transfer<IO>>, TftpResult) {
        if mode == "mail" {
            (
                None,
                TftpResult::Done(Some(Packet::ERROR {
                    code: ErrorCode::NoUser,
                    msg: "".to_owned(),
                })),
            )
        } else if let Ok(fwrite) = self.io.create_new(&filename) {
            (
                Some(Transfer {
                    fread: None,
                    expected_block_num: 1,
                    sent_final: false,
                    fwrite: Some(fwrite),
                    complete: false,
                }),
                TftpResult::Reply(Packet::ACK(0)),
            )
        } else {
            (
                None,
                TftpResult::Done(Some(Packet::ERROR {
                    code: ErrorCode::FileExists,
                    msg: "".to_owned(),
                })),
            )
        }
    }

    fn handle_rrq(&mut self, filename: &str, mode: &str) -> (Option<Transfer<IO>>, TftpResult) {
        if mode == "mail" {
            (
                None,
                TftpResult::Done(Some(Packet::ERROR {
                    code: ErrorCode::NoUser,
                    msg: "".to_owned(),
                })),
            )
        } else if let Ok(mut fread) = self.io.open_read(filename) {
            let mut v = vec![];
            fread.read_512(&mut v).unwrap();
            (
                Some(Transfer {
                    fread: Some(fread),
                    expected_block_num: 1,
                    sent_final: v.len() < 512,
                    fwrite: None,
                    complete: false,
                }),
                TftpResult::Reply(Packet::DATA {
                    block_num: 1,
                    data: v,
                }),
            )
        } else {
            (
                None,
                TftpResult::Done(Some(Packet::ERROR {
                    code: ErrorCode::FileNotFound,
                    msg: "".to_owned(),
                })),
            )
        }
    }
}

impl<IO: IOAdapter> Transfer<IO> {
    pub(crate) fn rx(&mut self, packet: Packet) -> TftpResult {
        match packet {
            Packet::ACK(ack_block) => self.handle_ack(ack_block),
            Packet::DATA { block_num, data } => self.handle_data(block_num, data),
            Packet::ERROR { .. } => {
                self.complete = true;
                TftpResult::Done(None)
            }
            _ => TftpResult::Err(TftpError::TransferAlreadyRunning),
        }
    }

    fn handle_ack(&mut self, ack_block: u16) -> TftpResult {
        if self.fwrite.is_some() {
            self.complete = true;
            TftpResult::Done(Some(Packet::ERROR {
                code: ErrorCode::IllegalTFTP,
                msg: "".to_owned(),
            }))
        } else if self.complete {
            TftpResult::Done(None)
        } else if ack_block == self.expected_block_num.wrapping_sub(1) {
            TftpResult::Repeat
        } else if ack_block != self.expected_block_num {
            self.complete = true;
            TftpResult::Done(Some(Packet::ERROR {
                code: ErrorCode::UnknownID,
                msg: "Incorrect block num in ACK".to_owned(),
            }))
        } else if self.sent_final {
            self.complete = true;
            TftpResult::Done(None)
        } else {
            let mut v = vec![];
            self.fread.as_mut().unwrap().read_512(&mut v).unwrap();
            self.sent_final = v.len() < 512;
            self.expected_block_num = self.expected_block_num.wrapping_add(1);
            TftpResult::Reply(Packet::DATA {
                block_num: self.expected_block_num,
                data: v,
            })
        }
    }

    fn handle_data(&mut self, block_num: u16, data: Vec<u8>) -> TftpResult {
        if self.fread.is_some() {
            self.complete = true;
            TftpResult::Done(Some(Packet::ERROR {
                code: ErrorCode::IllegalTFTP,
                msg: "".to_owned(),
            }))
        } else if self.complete {
            TftpResult::Done(None)
        } else if block_num != self.expected_block_num {
            self.complete = true;
            TftpResult::Done(Some(Packet::ERROR {
                code: ErrorCode::IllegalTFTP,
                msg: "Data packet lost".to_owned(),
            }))
        } else {
            self.fwrite
                .as_mut()
                .unwrap()
                .write_all(data.as_slice())
                .unwrap();
            self.expected_block_num = block_num.wrapping_add(1);
            if data.len() < 512 {
                self.complete = true;
                TftpResult::Done(Some(Packet::ACK(block_num)))
            } else {
                TftpResult::Reply(Packet::ACK(block_num))
            }
        }
    }
}
