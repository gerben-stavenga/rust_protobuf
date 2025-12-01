#![feature(likely_unlikely)]

pub mod base;
pub mod decoding;
pub mod encoding;
pub mod repeated_field;
pub mod wire;

pub(crate) mod utils;

mod test;

pub trait Protobuf {
    fn encoding_table() -> &'static [encoding::TableEntry];
    fn decoding_table() -> &'static decoding::Table;

    fn as_object(&self) -> &base::Object {
        unsafe { &*(self as *const Self as *const base::Object) }
    }

    fn as_object_mut(&mut self) -> &mut base::Object {
        unsafe { &mut *(self as *mut Self as *mut base::Object) }
    }
}

#[must_use]
pub fn parse_flat<const STACK_DEPTH: usize>(obj: &mut impl Protobuf, buf: &[u8]) -> bool {
    let mut parser = decoding::ResumeableParse::<STACK_DEPTH>::new(obj, isize::MAX);
    if !parser.resume(buf) {
        return false;
    }
    parser.finish()
}

pub fn parse_from_bufread<const STACK_DEPTH: usize>(
    obj: &mut impl Protobuf,
    reader: &mut impl std::io::BufRead,
) -> anyhow::Result<()> {
    let mut parser = decoding::ResumeableParse::<STACK_DEPTH>::new(obj, isize::MAX);
    let mut len = 0;
    loop {
        reader.consume(len);
        let buffer = reader.fill_buf()?;
        len = buffer.len();
        if len == 0 {
            break;
        }
        if !parser.resume(buffer) {
            return Err(anyhow::anyhow!("parse error"));
        }
    }
    if !parser.finish() {
        return Err(anyhow::anyhow!("parse error"));
    }
    Ok(())
}

pub fn parse_from_read<const STACK_DEPTH: usize>(
    obj: &mut impl Protobuf,
    reader: &mut impl std::io::Read,
) -> anyhow::Result<()> {
    let mut buf_reader = std::io::BufReader::new(reader);
    parse_from_bufread::<STACK_DEPTH>(obj, &mut buf_reader)
}

#[cfg(test)]
mod tests {
    use crate::test;

    use super::*;

    const BUFFER: [u8; 38] = [
        // x varint 0
        0o10, 1, // y fixed 64, 2
        0o21, 2, 0, 0, 0, 0, 0, 0, 0, // z length delimted 11
        0o32, 21, b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd', b'!', b'1',
        b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', // child is length delimited 34
        0o42, 2, 0o10, 2,
    ];

    #[test]
    fn test_resumeable_parse() {
        let mut test = test::Test::default();

        assert!(parse_flat::<100>(&mut test, &BUFFER));

        println!("{:?} {:?}", &test, test.child1());
        std::mem::forget(test);
    }

    // disable test temporarily
    #[test]
    fn test_resumeable_encode() {
        let mut test = test::Test::default();

        test.set_x(1);
        test.set_y(2);
        test.set_z(b"Hello World!123456789");
        let child = test.child1_mut();
        child.set_x(2);

        let mut buffer = [0u8; 64];

        let written = encoding::encode_flat::<100>(&test, &mut buffer).unwrap();
        assert_eq!(written, &BUFFER);
    }
}
