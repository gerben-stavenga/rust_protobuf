#![feature(likely_unlikely, allocator_api)]

pub mod arena;
pub mod base;
pub mod containers;
pub mod wire;

pub mod utils;

pub mod decoding;
pub mod encoding;
pub mod descriptor;

// #[cfg(feature = "serde_support")]
// pub mod serde;

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
    fn decode_flat<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        buf: &[u8],
    ) -> bool {
        let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self, isize::MAX);
        if !decoder.resume(buf, arena) {
            return false;
        }
        decoder.finish(arena)
    }

    fn decode<'a, E: std::error::Error + Send + Sync + 'static>(
        &mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> Result<Option<&'a [u8]>, E>,
    ) -> anyhow::Result<()> {
        let mut decoder = decoding::ResumeableDecode::<32>::new(self, isize::MAX);
        loop {
            let Some(buffer) = provider()? else {
                break;
            };
            if !decoder.resume(buffer, arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
        }
        if !decoder.finish(arena) {
            return Err(anyhow::anyhow!("decode error"));
        }
        Ok(())
    }

    fn async_decode<'a, E: std::error::Error + Send + Sync + 'static, F>(
        &'a mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> F,
    ) -> impl std::future::Future<Output = anyhow::Result<()>>
    where
        F: std::future::Future<Output = Result<Option<&'a [u8]>, E>> + 'a,
    {
        async move {
            let mut decoder = decoding::ResumeableDecode::<32>::new(self, isize::MAX);
            loop {
                let Some(buffer) = provider().await? else {
                    break;
                };
                if !decoder.resume(buffer, arena) {
                    return Err(anyhow::anyhow!("decode error"));
                }
            }
            if !decoder.finish(arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
            Ok(())
        }
    }

    fn decode_from_bufread<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        reader: &mut impl std::io::BufRead,
    ) -> anyhow::Result<()> {
        let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self, isize::MAX);
        loop {
            let buffer = reader.fill_buf()?;
            let len = buffer.len();
            if len == 0 {
                break;
            }
            if !decoder.resume(buffer, arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
            reader.consume(len);
        }
        if !decoder.finish(arena) {
            return Err(anyhow::anyhow!("decode error"));
        }
        Ok(())
    }

    fn decode_from_read<const STACK_DEPTH: usize>(
        &mut self,
        arena: &mut crate::arena::Arena,
        reader: &mut impl std::io::Read,
    ) -> anyhow::Result<()> {
        let mut buf_reader = std::io::BufReader::new(reader);
        self.decode_from_bufread::<STACK_DEPTH>(arena, &mut buf_reader)
    }

    fn decode_from_async_bufread<'a, const STACK_DEPTH: usize>(
        &'a mut self,
        arena: &'a mut crate::arena::Arena<'a>,
        reader: &mut (impl futures::io::AsyncBufRead + Unpin),
    ) -> impl std::future::Future<Output = anyhow::Result<()>> {
        use futures::io::AsyncBufReadExt;

        async move {
            let mut decoder = decoding::ResumeableDecode::<STACK_DEPTH>::new(self, isize::MAX);
            loop {
                let buffer = reader.fill_buf().await?;
                let len = buffer.len();
                if len == 0 {
                    break;
                }
                if !decoder.resume(buffer, arena) {
                    return Err(anyhow::anyhow!("decode error"));
                }
                reader.consume_unpin(len);
            }
            if !decoder.finish(arena) {
                return Err(anyhow::anyhow!("decode error"));
            }
            Ok(())
        }
    }

    fn decode_from_async_read<'a, const STACK_DEPTH: usize>(
        &'a mut self,
        arena: &'a mut crate::arena::Arena<'a>,
        reader: &mut (impl futures::io::AsyncRead + Unpin),
    ) -> impl std::future::Future<Output = anyhow::Result<()>> {
        async move {
            let mut buf_reader = futures::io::BufReader::new(reader);
            self.decode_from_async_bufread::<STACK_DEPTH>(arena, &mut buf_reader)
                .await
        }
    }

    fn encode_flat<'a, const STACK_DEPTH: usize>(
        &self,
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

#[cfg(any(test, feature = "bench"))]
pub mod tests {
    pub mod test;

    use super::*;

    use prost::Message;
    pub mod prost_gen {
        include!(concat!(env!("OUT_DIR"), "/_.rs"));
    }

    pub fn make_small_prost() -> prost_gen::Test {
        prost_gen::Test {
            x: Some(42),
            y: Some(0xDEADBEEF),
            z: None,
            child1: None,
            child2: None,
            nested_message: vec![],
        }
    }

    pub fn make_medium_prost() -> prost_gen::Test {
        prost_gen::Test {
            x: Some(42),
            y: Some(0xDEADBEEF),
            z: Some(b"Hello World! This is a test string with some content.".to_vec()),
            child1: Some(Box::new(prost_gen::Test {
                x: Some(123),
                y: Some(456),
                z: None,
                child1: None,
                child2: None,
                nested_message: vec![],
            })),
            child2: None,
            nested_message: vec![],
        }
    }

    pub fn make_large_prost() -> prost_gen::Test {
        prost_gen::Test {
            x: Some(42),
            y: Some(0xDEADBEEF),
            z: Some(b"Hello World!".to_vec()),
            child1: None,
            child2: None,
            nested_message: (0..100)
                .map(|i| prost_gen::test::NestedMessage {
                    x: Some(i),
                    recursive: None,
                })
                .collect(),
        }
    }

    pub fn encode_prost(msg: &prost_gen::Test) -> Vec<u8> {
        let mut buf = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut buf).unwrap();
        buf
    }

    pub fn make_protocrap(msg: &prost_gen::Test, arena: &mut crate::arena::Arena) -> test::Test {
        let mut protocrap_msg = test::Test::default();
        let data = encode_prost(msg);
        assert!(protocrap_msg.decode_flat::<32>(arena, &data));
        protocrap_msg
    }

    fn assert_roundtrip(msg: prost_gen::Test) {
        let data = encode_prost(&msg);

        let mut arena = crate::arena::Arena::new(&std::alloc::Global);
        let mut protocrap_msg = test::Test::default();
        assert!(protocrap_msg.decode_flat::<32>(&mut arena, &data));

        let mut buffer = vec![0u8; data.len()];
        let written = protocrap_msg
            .encode_flat::<32>(&mut buffer)
            .expect("msg should encode");
        println!("Protocrap encoded data: {:x?}", &written);
        println!("Protobuf encoded data : {:x?}", &data);
        assert_eq!(written.len(), data.len());

        let decoded_prost = prost_gen::Test::decode(&written[..]).expect("should decode");
        let encoded_data = encode_prost(&decoded_prost);
        assert_eq!(encoded_data, data);
    }

    #[test]
    fn test_small_roundtrips() {
        assert_roundtrip(make_small_prost());
    }

    #[test]
    fn test_medium_roundtrips() {
        assert_roundtrip(make_medium_prost());
    }

    #[test]
    fn test_large_roundtrips() {
        assert_roundtrip(make_large_prost());
    }
}
