use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::iter::Take;
use packet::{ErrorCode, Packet};
use tftp_proto::*;

#[test]
fn initial_ack_err() {
    let iof = TestIoFactory::new();
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::ACK(0));
    assert_eq!(res, Err(TftpError::NotIniatingPacket));
    assert!(xfer.is_none());
}

#[test]
fn initial_data_err() {
    let iof = TestIoFactory::new();
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::DATA {
        block_num: 1,
        data: vec![],
    });
    assert_eq!(res, Err(TftpError::NotIniatingPacket));
    assert!(xfer.is_none());
}

#[test]
fn rrq_no_file_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR { 
            code: ErrorCode::FileNotFound , ..
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn rrq_mail_gets_error() {
    let (mut server, file, _) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "mail".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "".into(),
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn rrq_netascii_gets_error() {
    let (mut server, file, _) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "netascii".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NotDefined,
            msg: "".into(),
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn wrq_netascii_gets_error() {
    let (mut server, file, _) = wrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "netascii".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NotDefined,
            msg: "".into(),
        })
    );
    assert!(xfer.is_none());
}

fn rrq_fixture(file_size: usize) -> (TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), file_size);
    iof.server_present_files.insert(file.clone());
    let serv = TftpServerProto::new(iof, Default::default());
    let file_bytes = ByteGen::new(&file);
    (serv, file, file_bytes)
}

#[test]
fn rrq_small_file_ack_end() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert!(!xfer.is_done());
    assert_eq!(xfer.rx(Packet::ACK(1)), TftpResult::Done(None));
    assert!(xfer.is_done());
    assert_eq!(xfer.rx(Packet::ACK(0)), TftpResult::Done(None));
}

#[test]
fn rrq_1_block_file() {
    let (mut server, file, mut file_bytes) = rrq_fixture(512);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: vec![],
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_small_file_ack_wrong_block() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(2)),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::UnknownID,
            msg: "Incorrect block num in ACK".into(),
        }))
    );
    assert_eq!(xfer.rx(Packet::ACK(0)), TftpResult::Done(None));
}

#[test]
fn rrq_small_file_reply_with_data_illegal() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            data: vec![],
            block_num: 1,
        }),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            msg: "".into(),
        }))
    );
    assert_eq!(xfer.rx(Packet::ACK(0)), TftpResult::Done(None));
}

#[test]
fn double_rrq() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::RRQ {
            filename: file,
            mode: "octet".into(),
        }),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn rrq_2_blocks_ok() {
    let (mut server, file, mut file_bytes) = rrq_fixture(612);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(100),
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_2_blocks_second_lost_ack_repeat_ok() {
    let (mut server, file, mut file_bytes) = rrq_fixture(612);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(100),
        })
    );
    // assuming the second data got lost, and the client re-acks the first data
    assert_eq!(xfer.rx(Packet::ACK(1)), TftpResult::Repeat);
    assert_eq!(xfer.rx(Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_large_file_blocknum_wraparound() {
    let size_bytes = 512 * 70_000 + 85;
    let (mut server, file, mut file_bytes) = rrq_fixture(size_bytes);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();

    let mut block = 1u16;
    // loop for one less because we already got the first DATA above
    for _ in 0..(size_bytes / 512) - 1 {
        let new_block = block.wrapping_add(1);
        assert_eq!(
            xfer.rx(Packet::ACK(block)),
            TftpResult::Reply(Packet::DATA {
                block_num: new_block,
                data: file_bytes.gen(512),
            })
        );
        block = new_block;
    }

    let new_block = block.wrapping_add(1);
    assert_eq!(
        xfer.rx(Packet::ACK(block)),
        TftpResult::Reply(Packet::DATA {
            block_num: new_block,
            data: file_bytes.gen(85),
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(new_block)), TftpResult::Done(None));
}

#[test]
fn rrq_small_file_wrq_already_running() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::WRQ {
            filename: file,
            mode: "octet".into(),
        }),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn rrq_small_file_err_kills_transfer() {
    let (mut server, file, mut file_bytes) = rrq_fixture(612);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ERROR {
            code: ErrorCode::DiskFull,
            msg: "".into(),
        }),
        TftpResult::Done(None)
    );
    assert_eq!(xfer.rx(Packet::ACK(0)), TftpResult::Done(None));
}

#[test]
fn wrq_already_exists_error() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), 132);
    iof.server_present_files.insert(file.clone());
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::FileExists,
            msg: "".into(),
        })
    );
    assert!(xfer.is_none());
}

fn wrq_fixture(file_size: usize) -> (TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), file_size);
    let serv = TftpServerProto::new(iof, Default::default());
    let file_bytes = ByteGen::new(&file);
    (serv, file, file_bytes)
}

fn wrq_fixture_early_termination(
    file_size: usize,
) -> (TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), file_size);
    iof.enforce_full_write = false;
    let serv = TftpServerProto::new(iof, Default::default());
    let file_bytes = ByteGen::new(&file);
    (serv, file, file_bytes)
}

#[test]
fn wrq_mail_gets_error() {
    let (mut server, file, _) = wrq_fixture(200);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "mail".into(),
    });
    assert_eq!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "".into(),
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn wrq_small_file_ack_end() {
    let (mut server, file, mut file_bytes) = wrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert!(!xfer.is_done());
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        }),
        TftpResult::Done(Some(Packet::ACK(1)))
    );
    assert!(xfer.is_done());
}

#[test]
fn wrq_1_block_file() {
    let (mut server, file, mut file_bytes) = wrq_fixture(512);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "octet".into(),
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        }),
        TftpResult::Reply(Packet::ACK(1))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: vec![],
        }),
        TftpResult::Done(Some(Packet::ACK(2)))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: vec![],
        }),
        TftpResult::Done(None)
    );
}

#[test]
fn wrq_small_file_reply_with_ack_illegal() {
    let (mut server, file, mut file_bytes) = wrq_fixture(512);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "octet".to_owned(),
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        }),
        TftpResult::Reply(Packet::ACK(1))
    );
    assert_eq!(
        xfer.rx(Packet::ACK(3)),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            msg: "".to_owned(),
        }))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: vec![],
        }),
        TftpResult::Done(None)
    );
}

#[test]
fn wrq_small_file_block_id_not_1_err() {
    let (mut server, file, mut file_bytes) = wrq_fixture_early_termination(132);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "octet".to_owned(),
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(132),
        }),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            msg: "Data packet lost".to_owned(),
        }))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: vec![],
        }),
        TftpResult::Done(None)
    );
}

#[test]
fn wrq_large_file_blocknum_wraparound() {
    let size_bytes = 512 * 70_000 + 85;
    let (mut server, file, mut file_bytes) = wrq_fixture(size_bytes);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: "octet".to_owned(),
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();

    let mut block_num = 1;
    for _ in 0..(size_bytes / 512) {
        assert_eq!(
            xfer.rx(Packet::DATA {
                block_num,
                data: file_bytes.gen(512),
            }),
            TftpResult::Reply(Packet::ACK(block_num))
        );
        block_num = block_num.wrapping_add(1);
    }
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num,
            data: file_bytes.gen(85),
        }),
        TftpResult::Done(Some(Packet::ACK(block_num)))
    );
}

#[test]
fn policy_readonly() {
    let mut iof = TestIoFactory::new();
    let amt = 100;
    let file_a = "file_a".to_owned();
    let file_b = "file_b".to_owned();
    iof.possible_files.insert(file_a.clone(), amt);
    iof.server_present_files.insert(file_a.clone());
    iof.possible_files.insert(file_b.clone(), amt);

    let proxy = IOPolicyProxy::new(
        iof,
        IOPolicyCfg {
            readonly: true,
            path: None,
        },
    );
    let mut v = vec![];
    assert_eq!(
        proxy
            .open_read(&file_a)
            .unwrap()
            .read_to_end(&mut v)
            .unwrap(),
        amt
    );
    assert_eq!(v.len(), amt);

    let mut proxy = proxy;
    if let Ok(_) = proxy.create_new(&file_b) {
        panic!("create should not succeed");
    }
}

#[test]
fn policy_remap_directory() {
    // TODO: improve this test
    let mut iof = TestIoFactory::new();
    let amt = 100;
    let file_a = "the_new_path/file_a".to_owned();
    let file_b = "the_new_path/file_b".to_owned();
    iof.possible_files.insert(file_a.clone(), amt);
    iof.possible_files.insert(file_b.clone(), amt);
    iof.server_present_files.insert(file_a.clone());
    iof.enforce_full_write = false;

    let mut proxy = IOPolicyProxy::new(
        iof,
        IOPolicyCfg {
            readonly: false,
            path: Some("the_new_path".into()),
        },
    );
    assert!(proxy.open_read("the_new_path/file_a").is_err());
    assert!(proxy.open_read("file_a").is_ok());

    assert!(proxy.create_new("the_new_path/file_b").is_err());
    assert!(proxy.create_new("file_b").is_ok());
}

struct TestIoFactory {
    server_present_files: HashSet<String>,
    possible_files: HashMap<String, usize>,
    enforce_full_write: bool,
}
impl TestIoFactory {
    fn new() -> Self {
        TestIoFactory {
            server_present_files: HashSet::new(),
            possible_files: HashMap::new(),
            enforce_full_write: true,
        }
    }
}
impl IOAdapter for TestIoFactory {
    type R = GeneratingReader;
    type W = ExpectingWriter;
    fn open_read(&self, filename: &str) -> io::Result<Self::R> {
        if self.server_present_files.contains(filename) {
            let size = *self.possible_files.get(filename).unwrap();
            Ok(GeneratingReader::new(filename, size))
        } else if !self.possible_files.contains_key(filename) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unexpected file",
            ))
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "test file not found",
            ))
        }
    }
    fn create_new(&mut self, filename: &str) -> io::Result<ExpectingWriter> {
        if self.server_present_files.contains(filename) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test file already there",
            ))
        } else if !self.possible_files.contains_key(filename) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unexpected file",
            ))
        } else {
            self.server_present_files.insert(filename.into());
            let size = *self.possible_files.get(filename).unwrap();
            Ok(ExpectingWriter::new(
                filename,
                size,
                self.enforce_full_write,
            ))
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
        self.take(n).collect()
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
    enforce_full_write: bool,
}
impl ExpectingWriter {
    fn new(s: &str, amt: usize, enforce_full_write: bool) -> Self {
        Self {
            gen: ByteGen::new(s).take(amt),
            enforce_full_write,
        }
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
        if self.enforce_full_write {
            let (_, sup) = self.gen.size_hint();
            assert_eq!(
                sup,
                Some(0),
                "writer destroyed before all bytes were written"
            );
        }
    }
}
