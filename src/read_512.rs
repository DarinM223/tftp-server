use std::io;
use std::io::*;

pub(crate) trait Read512 {
    fn read_512(&mut self, buf: &mut Vec<u8>) -> io::Result<usize>;
}

impl<T> Read512 for T
where
    T: Read,
{
    fn read_512(&mut self, mut buf: &mut Vec<u8>) -> io::Result<usize> {
        self.take(512).read_to_end(&mut buf)
    }
}
