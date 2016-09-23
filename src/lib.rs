#![feature(question_mark)]

#[macro_use]
extern crate log;

extern crate env_logger;
extern crate byteorder;
extern crate mio;
extern crate rand;

pub mod packet;
pub mod server;
