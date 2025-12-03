#![feature(likely_unlikely, allocator_api)]

pub mod arena;
pub mod base;
pub mod repeated_field;
pub mod wire;

pub mod utils;

mod decoding;
mod encoding;
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

pub trait ProtobufExt: Protobuf {
    #[must_use]
    fn parse_flat<const STACK_DEPTH: usize>(&mut self, buf: &[u8]) -> bool {
        let mut parser = decoding::ResumeableParse::<STACK_DEPTH>::new(self, isize::MAX);
        if !parser.resume(buf) {
            return false;
        }
        parser.finish()
    }

    fn parse<'a, E: std::error::Error + Send + Sync + 'static>(
        &mut self,
        provider: &'a mut impl FnMut() -> Result<Option<&'a [u8]>, E>,
    ) -> anyhow::Result<()> {
        let mut parser = decoding::ResumeableParse::<32>::new(self, isize::MAX);
        loop {
            let Some(buffer) = provider()? else {
                break;
            };
            if !parser.resume(buffer) {
                return Err(anyhow::anyhow!("parse error"));
            }
        }
        if !parser.finish() {
            return Err(anyhow::anyhow!("parse error"));
        }
        Ok(())
    }

    fn async_parse<'a, E: std::error::Error + Send + Sync + 'static, F>(
        &mut self,
        provider: &'a mut impl FnMut() -> F,
    ) -> impl std::future::Future<Output = anyhow::Result<()>>
    where
        F: std::future::Future<Output = Result<Option<&'a [u8]>, E>> + 'a,
    {
        async move {
            let mut parser = decoding::ResumeableParse::<32>::new(self, isize::MAX);
            loop {
                let Some(buffer) = provider().await? else {
                    break;
                };
                if !parser.resume(buffer) {
                    return Err(anyhow::anyhow!("parse error"));
                }
            }
            if !parser.finish() {
                return Err(anyhow::anyhow!("parse error"));
            }
            Ok(())
        }
    }

    fn parse_from_bufread<const STACK_DEPTH: usize>(
        &mut self,
        reader: &mut impl std::io::BufRead,
    ) -> anyhow::Result<()> {
        let mut parser = decoding::ResumeableParse::<STACK_DEPTH>::new(self, isize::MAX);
        loop {
            let buffer = reader.fill_buf()?;
            let len = buffer.len();
            if len == 0 {
                break;
            }
            if !parser.resume(buffer) {
                return Err(anyhow::anyhow!("parse error"));
            }
            reader.consume(len);
        }
        if !parser.finish() {
            return Err(anyhow::anyhow!("parse error"));
        }
        Ok(())
    }

    fn parse_from_read<const STACK_DEPTH: usize>(
        &mut self,
        reader: &mut impl std::io::Read,
    ) -> anyhow::Result<()> {
        let mut buf_reader = std::io::BufReader::new(reader);
        self.parse_from_bufread::<STACK_DEPTH>(&mut buf_reader)
    }

    fn parse_from_async_bufread<const STACK_DEPTH: usize>(
        &mut self,
        reader: &mut (impl futures::io::AsyncBufRead + Unpin),
    ) -> impl std::future::Future<Output = anyhow::Result<()>> {
        use futures::io::AsyncBufReadExt;

        async move {
            let mut parser = decoding::ResumeableParse::<STACK_DEPTH>::new(self, isize::MAX);
            loop {
                let buffer = reader.fill_buf().await?;
                let len = buffer.len();
                if len == 0 {
                    break;
                }
                if !parser.resume(buffer) {
                    return Err(anyhow::anyhow!("parse error"));
                }
                reader.consume_unpin(len);
            }
            if !parser.finish() {
                return Err(anyhow::anyhow!("parse error"));
            }
            Ok(())
        }
    }

    fn parse_from_async_read<const STACK_DEPTH: usize>(
        &mut self,
        reader: &mut (impl futures::io::AsyncRead + Unpin),
    ) -> impl std::future::Future<Output = anyhow::Result<()>> {
        async move {
            let mut buf_reader = futures::io::BufReader::new(reader);
            self.parse_from_async_bufread::<STACK_DEPTH>(&mut buf_reader)
                .await
        }
    }

    fn encode_flat<'a, const STACK_DEPTH: usize>(
        &mut self,
        buffer: &'a mut [u8],
    ) -> anyhow::Result<&'a [u8]> {
        let mut resumeable_encode = encoding::ResumeableEncode::<STACK_DEPTH>::new(self);
        let encoding::ResumeResult::Done(buf) = resumeable_encode
            .resume_encode(buffer)
            .ok_or(anyhow::anyhow!("Message tree too deep"))?
        else {
            return Err(anyhow::anyhow!("Buffer too small for message"));
        };
        Ok(buf)
    }
}

impl<T: Protobuf> ProtobufExt for T {}

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

        assert!(test.parse_flat::<100>(&BUFFER));

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

        let written = test.encode_flat::<100>(&mut buffer).unwrap();
        assert_eq!(written, &BUFFER);
    }
}
