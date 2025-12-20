#![feature(likely_unlikely, allocator_api)]
#![cfg_attr(not(feature = "std"), no_std)]

pub mod arena;
pub mod base;
pub mod containers;
pub mod wire;

pub mod utils;

pub mod decoding;
pub mod encoding;

use crate as protocrap;
include!("descriptor.pc.rs");

#[cfg(feature = "serde_support")]
pub mod serde;

pub trait Protobuf: Default {
    fn encoding_table() -> &'static [encoding::TableEntry];
    fn decoding_table() -> &'static decoding::Table;
    fn descriptor_proto() -> &'static google::protobuf::DescriptorProto::ProtoType {
        Self::decoding_table().descriptor
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
}

impl<T: Protobuf> ProtobufExt for T {}

#[cfg(test)]
mod tests {
    #[test]
    fn descriptor_accessors() {
        use crate::Protobuf;
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        let message_descriptor =
            crate::google::protobuf::DescriptorProto::ProtoType::descriptor_proto();
        let nested_descriptor =
            crate::google::protobuf::DescriptorProto::ExtensionRange::ProtoType::descriptor_proto();

        assert_eq!(file_descriptor.name(), "descriptor.proto");
        assert_eq!(message_descriptor.name(), "DescriptorProto");
        assert_eq!(nested_descriptor.name(), "ExtensionRange");
    }
}
