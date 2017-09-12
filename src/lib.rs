#![deny(warnings)]

#[macro_use]
extern crate log;

extern crate byteorder;
extern crate mio;
extern crate mio_more;

pub mod packet;
pub mod server;
mod tftp_proto;

#[cfg(test)]
mod tftp_proto_tests;
#[cfg(test)]
#[macro_use]
extern crate assert_matches;

mod read_512;
