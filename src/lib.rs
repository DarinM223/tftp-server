#![deny(warnings)]

#[macro_use]
extern crate log;

extern crate env_logger;
extern crate byteorder;
extern crate mio;
extern crate mio_more;

pub mod packet;
pub mod server;
mod tftp_proto;
mod tftp_proto_tests;
mod read_512;
