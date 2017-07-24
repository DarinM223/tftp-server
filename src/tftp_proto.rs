use mio::*;
use std::collections::HashMap;
use std::collections::hash_map::Entry::Occupied;
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
    /// The transfer token is not part of any ongoing transfer
    InvalidTransferToken,

    /// The transfer token is already part of an ongoing transfer,
    /// and cannot be used for a new transfer
    TransferAlreadyRunning,
}

struct Transfer<IO: IOAdapter> {
    fread: Option<IO::R>,
    sent_block_num: u16,
    sent_final: bool,
    fwrite: Option<IO::W>,
}

/// The TFTP protocol and filesystem usage implementation,
/// used as backend for a TFTP server
pub struct TftpServerProto<IO: IOAdapter> {
    io: IO,
    xfers: HashMap<Token, Transfer<IO>>,
}

impl<IO: IOAdapter> TftpServerProto<IO> {
    /// Creates a new instance with the provided IOAdapter
    pub(crate) fn new(io: IO) -> Self {
        TftpServerProto {
            io: io,
            xfers: HashMap::new(),
        }
    }

    /// Signals the protocol implementation that the connection
    /// associated with this token has timed out.
    /// The protocol may forget any state associated with that token.
    pub(crate) fn timeout(&mut self, token: Token) {
        self.xfers.remove(&token);
    }

    /// Signals the receipt of a packet.
    ///
    /// For RRQ and WRQ packets, the token must be a new one, not yet associated with current transfers.
    ///
    /// For DATA, ACK, and ERROR packets, the token must be the same one supplied with the initial RRQ/WRQ packet.
    ///
    /// The token will remain uniquely associated with its connection,
    /// until `TftpResult::Done` or `TftpResult::Err` is returned, or until `timeout` is called.
    pub(crate) fn recv(&mut self, token: Token, packet: Packet) -> TftpResult {
        match packet {
            Packet::RRQ { filename, mode } => {
                if mode == "mail" {
                    return TftpResult::Done(Some(Packet::ERROR {
                        code: ErrorCode::NoUser,
                        msg: "".to_owned(),
                    }));
                }
                if self.xfers.contains_key(&token) {
                    return TftpResult::Err(TftpError::TransferAlreadyRunning);
                }
                if let Ok(mut fread) = self.io.open_read(filename) {
                    let mut v = vec![];
                    fread.read_512(&mut v).unwrap();
                    self.xfers.insert(
                        token,
                        Transfer {
                            fread: Some(fread),
                            sent_block_num: 1,
                            sent_final: v.len() < 512,
                            fwrite: None,
                        },
                    );
                    TftpResult::Reply(Packet::DATA {
                        block_num: 1,
                        data: v,
                    })
                } else {
                    TftpResult::Done(Some(Packet::ERROR {
                        code: ErrorCode::FileNotFound,
                        msg: "".to_owned(),
                    }))
                }
            }
            Packet::ACK(ack_block) => {
                if let Occupied(mut xfer) = self.xfers.entry(token) {
                    if ack_block == xfer.get().sent_block_num.wrapping_sub(1) {
                        TftpResult::Repeat
                    } else if ack_block != xfer.get().sent_block_num {
                        xfer.remove_entry();
                        TftpResult::Done(Some(Packet::ERROR {
                            code: ErrorCode::UnknownID,
                            msg: "Incorrect block num in ACK".to_owned(),
                        }))
                    } else if xfer.get().sent_final {
                        xfer.remove_entry();
                        TftpResult::Done(None)
                    } else {
                        let xfer = xfer.get_mut();
                        let mut v = vec![];
                        xfer.fread.as_mut().unwrap().read_512(&mut v).unwrap();
                        xfer.sent_final = v.len() < 512;
                        xfer.sent_block_num = xfer.sent_block_num.wrapping_add(1);
                        TftpResult::Reply(Packet::DATA {
                            block_num: xfer.sent_block_num,
                            data: v,
                        })
                    }
                } else {
                    TftpResult::Err(TftpError::InvalidTransferToken)
                }
            }
            Packet::DATA { block_num, data } => {
                if let Occupied(mut xfer) = self.xfers.entry(token) {
                    if xfer.get().fwrite.is_some() {
                        xfer.get_mut().fwrite.as_mut().unwrap().write_all(data.as_slice()).unwrap();
                        return TftpResult::Done(Some(Packet::ACK(block_num)));
                    }
                }
                self.xfers.remove(&token);
                TftpResult::Done(Some(Packet::ERROR {
                    code: ErrorCode::IllegalTFTP,
                    msg: "".to_owned(),
                }))
            }
            Packet::WRQ { filename, mode } => {
                if self.xfers.contains_key(&token) {
                    return TftpResult::Err(TftpError::TransferAlreadyRunning);
                }
                if mode == "mail" {
                    return TftpResult::Done(Some(Packet::ERROR {
                        code: ErrorCode::NoUser,
                        msg: "".to_owned(),
                    }));
                }
                if let Ok(mut fwrite) = self.io.create_new(filename) {
                    self.xfers.insert(
                        token,
                        Transfer {
                            fread: None,
                            sent_block_num: 1,
                            sent_final: false,
                            fwrite: Some(fwrite),
                        },
                    );
                    TftpResult::Reply(Packet::ACK(0))
                } else {
                    TftpResult::Done(Some(Packet::ERROR {
                        code: ErrorCode::FileExists,
                        msg: "".to_owned(),
                    }))
                }
            }
            _ => TftpResult::Err(TftpError::InvalidTransferToken),
        }
    }
}
