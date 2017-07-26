#![cfg(test)]

use mio::*;
use std::borrow::BorrowMut;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::iter::Take;
use packet::{ErrorCode, Packet};
use server::IOAdapter;
use tftp_proto::*;

#[test]
fn initial_ack_err() {
    let iof = TestIoFactory::new();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(Token(1), Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn initial_data_err() {
    let iof = TestIoFactory::new();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::DATA {
                block_num: 1,
                data: vec![],
            },
        ),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_no_file_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::FileNotFound,
            msg: "".into(),
        }))
    );
}

#[test]
fn rrq_mail_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::RRQ {
                filename: file,
                mode: "mail".into(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "".into(),
        }))
    );
}

fn rrq_fixture(file_size: usize) -> (Token, TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), file_size);
    iof.server_files.insert(file.clone());
    let serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    (Token(26), serv, file, byte_gen)
}

#[test]
fn rrq_small_file_ack_end() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(132);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(132),
        })
    );
    assert_eq!(serv.recv(tk, Packet::ACK(1)), TftpResult::Done(None));
    assert_eq!(
        serv.recv(tk, Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_1_block_file() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(512);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(512),
        })
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: vec![],
        })
    );
    assert_eq!(serv.recv(tk, Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_small_file_ack_timeout_err() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(132);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(132),
        })
    );
    serv.timeout(tk);
    assert_eq!(
        serv.recv(tk, Packet::ACK(1)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_small_file_ack_wrong_block() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(132);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(132),
        })
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(2)),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::UnknownID,
            msg: "Incorrect block num in ACK".into(),
        }))
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_small_file_reply_with_data_illegal() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(132);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(132),
        })
    );
    assert_eq!(
        serv.recv(
            tk,
            Packet::DATA {
                data: vec![],
                block_num: 1,
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            msg: "".into(),
        }))
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn double_rrq() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(132);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(132),
        })
    );
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn rrq_2_blocks_ok() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(612);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(512),
        })
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: byte_gen.gen(100),
        })
    );
    assert_eq!(serv.recv(tk, Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_2_blocks_second_lost_ack_repeat_ok() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(612);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(512),
        })
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: byte_gen.gen(100),
        })
    );
    // assuming the second data got lost, and the client re-acks the first data
    assert_eq!(serv.recv(tk, Packet::ACK(1)), TftpResult::Repeat);
    assert_eq!(serv.recv(tk, Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_large_file_blocknum_wraparound() {
    let size_bytes = 512 * 70_000 + 85;
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(size_bytes);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(512),
        })
    );

    let mut block = 1u16;
    for _ in 0..(size_bytes / 512) - 1
    //one less because we already got the first DATA above
    {
        let new_block = block.wrapping_add(1);
        assert_eq!(
            serv.recv(tk, Packet::ACK(block)),
            TftpResult::Reply(Packet::DATA {
                block_num: new_block,
                data: byte_gen.gen(512),
            })
        );
        block = new_block;
    }

    let new_block = block.wrapping_add(1);
    assert_eq!(
        serv.recv(tk, Packet::ACK(block)),
        TftpResult::Reply(Packet::DATA {
            block_num: new_block,
            data: byte_gen.gen(85),
        })
    );
    assert_eq!(
        serv.recv(tk, Packet::ACK(new_block)),
        TftpResult::Done(None)
    );
}

#[test]
fn rrq_small_file_wrq_already_running() {
    let (tk, mut serv, file, mut byte_gen) = rrq_fixture(132);
    assert_eq!(
        serv.recv(
            tk,
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.gen(132),
        })
    );
    assert_eq!(
        serv.recv(
            tk,
            Packet::WRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn wrq_already_exists_error() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::FileExists,
            msg: "".into(),
        }))
    );
}

#[test]
fn wrq_mail_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "mail".into(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "".into(),
        }))
    );
}

#[test]
fn wrq_small_file_ack_end() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    let mut serv = TftpServerProto::new(iof);
    let mut byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::ACK(0))
    );
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::DATA {
                block_num: 1,
                data: byte_gen.gen(132),
            },
        ),
        TftpResult::Done(Some(Packet::ACK(1)))
    );
}

#[test]
fn wrq_1_block_file() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 512);
    let mut serv = TftpServerProto::new(iof);
    let mut byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "octet".into(),
            },
        ),
        TftpResult::Reply(Packet::ACK(0))
    );
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::DATA {
                block_num: 1,
                data: byte_gen.gen(512),
            },
        ),
        TftpResult::Reply(Packet::ACK(1))
    );
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::DATA {
                block_num: 2,
                data: vec![],
            },
        ),
        TftpResult::Done(Some(Packet::ACK(2)))
    );
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::DATA {
                block_num: 2,
                data: vec![],
            },
        ),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

struct TestIoFactory {
    server_files: HashSet<String>,
    files: HashMap<String, usize>,
}
impl TestIoFactory {
    fn new() -> Self {
        TestIoFactory {
            server_files: HashSet::new(),
            files: HashMap::new(),
        }
    }
}
impl IOAdapter for TestIoFactory {
    type R = GeneratingReader;
    type W = ExpectingWriter;
    fn open_read(&self, filename: &str) -> io::Result<Self::R> {
        if self.server_files.contains(filename) {
            let size = *self.files.get(filename).unwrap();
            Ok(GeneratingReader::new(filename, size))
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "test file not found",
            ))
        }
    }
    fn create_new(&mut self, filename: &str) -> io::Result<ExpectingWriter> {
        if self.server_files.contains(filename) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test file already there",
            ))
        } else {
            self.server_files.insert(filename.into());
            let size = *self.files.get(filename).unwrap();
            Ok(ExpectingWriter::new(filename, size))
        }
    }
}

struct ByteGen {
    crt: u8,
    count: u8,
}
impl ByteGen {
    // pseudorandom, just so we get different values for each file
    fn new(s: &str) -> Self {
        let mut v = 0;
        for b in s.bytes() {
            v ^= b;
        }
        ByteGen { crt: v, count: 0 }
    }
    fn gen(&mut self, n: usize) -> Vec<u8> {
        self.borrow_mut().take(n).collect()
    }
}
impl Iterator for ByteGen {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        self.crt = self.crt.wrapping_add(1).wrapping_mul(self.count);
        self.count = self.count.wrapping_add(1);
        Some(self.crt)
    }
}

struct GeneratingReader {
    gen: Take<ByteGen>,
}
impl GeneratingReader {
    fn new(s: &str, amt: usize) -> Self {
        Self { gen: ByteGen::new(s).take(amt) }
    }
}
impl Read for GeneratingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // TODO: write this more concisely
        let mut read = 0;
        for e in buf {
            if let Some(v) = self.gen.next() {
                *e = v;
                read += 1;
            } else {
                break;
            }
        }
        Ok(read)
    }
}

struct ExpectingWriter {
    gen: Take<ByteGen>,
}
impl ExpectingWriter {
    fn new(s: &str, amt: usize) -> Self {
        Self { gen: ByteGen::new(s).take(amt) }
    }
}
impl Write for ExpectingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: write this more concisely
        let mut wrote = 0;
        for b in buf {
            if let Some(v) = self.gen.next() {
                assert_eq!(*b, v);
                wrote += 1;
            } else {
                panic!("wrote more than expected");
            }
        }
        Ok(wrote)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for ExpectingWriter {
    fn drop(&mut self) {
        let (_, sup) = self.gen.size_hint();
        assert_eq!(
            sup,
            Some(0),
            "writer destroyed before all bytes were written"
        );
    }
}
