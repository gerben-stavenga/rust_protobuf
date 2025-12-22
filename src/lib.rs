#![feature(likely_unlikely, allocator_api)]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod arena;
pub mod base;
pub mod containers;
pub mod wire;

pub mod utils;

pub mod decoding;
pub mod encoding;
pub mod reflection;
pub mod tables;

use crate as protocrap;
include!("descriptor.pc.rs");

#[cfg(feature = "serde_support")]
pub mod serde;

pub trait Protobuf: Default + core::fmt::Debug {
    fn table() -> &'static tables::Table;
    fn descriptor_proto() -> &'static google::protobuf::DescriptorProto::ProtoType {
        Self::table().descriptor
    }

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

    fn decode<'a, E: core::error::Error + Send + Sync + 'static>(
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

    fn async_decode<'a, E: core::error::Error + Send + Sync + 'static, F>(
        &'a mut self,
        arena: &mut crate::arena::Arena,
        provider: &'a mut impl FnMut() -> F,
    ) -> impl core::future::Future<Output = anyhow::Result<()>>
    where
        F: core::future::Future<Output = Result<Option<&'a [u8]>, E>> + 'a,
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

    #[cfg(feature = "std")]
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

    #[cfg(feature = "std")]
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
    ) -> impl core::future::Future<Output = anyhow::Result<()>> {
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
    ) -> impl core::future::Future<Output = anyhow::Result<()>> {
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

    #[cfg(feature = "std")]
    fn encode_vec<const STACK_DEPTH: usize>(&self) -> anyhow::Result<Vec<u8>> {
        let mut buffer = vec![0u8; 1024];
        let mut stack = Vec::new();
        let mut resumeable_encode = encoding::ResumeableEncode::<STACK_DEPTH>::new(self);
        loop {
            match resumeable_encode
                .resume_encode(&mut buffer)
                .ok_or(anyhow::anyhow!("Message tree too deep"))?
            {
                encoding::ResumeResult::Done(buf) => {
                    let len = buf.len();
                    let end = buffer.len();
                    let start = end - len;
                    buffer.copy_within(start..end, 0);
                    buffer.truncate(len);
                    break;
                }
                encoding::ResumeResult::NeedsMoreBuffer => {
                    let len = buffer.len().min(1024 * 1024);
                    stack.push(core::mem::take(&mut buffer));
                    buffer = vec![0u8; len * 2];
                }
            };
        }
        while let Some(old_buffer) = stack.pop() {
            buffer.extend_from_slice(&old_buffer);
        }
        Ok(buffer)
    }
}

impl<T: Protobuf> ProtobufExt for T {}

pub mod tests {
    use crate::{Protobuf, ProtobufExt};

    pub fn assert_roundtrip<T: Protobuf>(msg: &T) {
        let data = msg.encode_vec::<32>().expect("msg should encode");

        let mut arena = crate::arena::Arena::new(&std::alloc::Global);
        let mut roundtrip_msg = T::default();
        assert!(roundtrip_msg.decode_flat::<32>(&mut arena, &data));

        println!("Roundtrip message: {:#?}", roundtrip_msg);

        let roundtrip_data = roundtrip_msg.encode_vec::<32>().expect("msg should encode");

        assert_eq!(roundtrip_data, data);
    }

    #[test]
    fn descriptor_accessors() {
        use crate::Protobuf;
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        let message_descriptor =
            crate::google::protobuf::DescriptorProto::ProtoType::descriptor_proto();
        let nested_descriptor =
            crate::google::protobuf::DescriptorProto::ExtensionRange::ProtoType::descriptor_proto();

        assert_eq!(file_descriptor.name(), "proto/descriptor.proto");
        assert_eq!(message_descriptor.name(), "DescriptorProto");
        assert_eq!(nested_descriptor.name(), "ExtensionRange");
    }

    #[test]
    fn file_descriptor_roundtrip() {
        use crate::ProtobufExt;
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();

        let mut buffer1 = vec![0u8; 100000];
        let encoded = file_descriptor.encode_flat::<32>(&mut buffer1).unwrap();

        println!("Encoded descriptor.proto ({} bytes)", encoded.len());
        println!("syntax: {}, edition: {:?}", file_descriptor.syntax(), file_descriptor.edition());

        let mut message = crate::google::protobuf::FileDescriptorProto::ProtoType::default();
        let mut arena = crate::arena::Arena::new(&std::alloc::Global);
        assert!(message.decode_flat::<32>(&mut arena, encoded));

        let mut buffer2 = vec![0u8; 100000];
        let re_encoded = message.encode_flat::<32>(&mut buffer2).unwrap();
        assert_eq!(encoded, re_encoded);
    }
}
