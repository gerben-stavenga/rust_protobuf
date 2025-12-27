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

// One would like to implement Default and Debug for all T: Protobuf via a blanket impl,
// but that is not allowed because Default and Debug are not local to this crate.
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

/// Read-only protobuf operations (encode, serialize, inspect).
/// The lifetime parameter `'pool` refers to the descriptor/table pool lifetime.
pub trait ProtobufRef<'pool> {
    fn table(&self) -> &'pool tables::Table;

    fn descriptor(&self) -> &'pool google::protobuf::DescriptorProto::ProtoType {
        self.table().descriptor
    }

    fn as_object(&self) -> &base::Object;

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

/// Mutable protobuf operations (decode, deserialize).
/// Extends ProtobufRef with mutation capabilities.
pub trait ProtobufMut<'pool>: ProtobufRef<'pool> {
    fn as_object_mut(&mut self) -> &mut base::Object;

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

    #[cfg(feature = "serde_support")]
    fn serde_deserialize<'arena, 'alloc, 'de, D>(
        &'de mut self,
        arena: &'arena mut crate::arena::Arena<'alloc>,
        deserializer: D,
    ) -> Result<(), D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        let table = self.table();
        serde::serde_deserialize_struct(self.as_object_mut(), table, arena, deserializer)
    }
}

// Blanket impl for static protobuf types
impl<T: Protobuf> ProtobufRef<'static> for T {
    fn table(&self) -> &'static tables::Table {
        T::table()
    }

    fn as_object(&self) -> &base::Object {
        <Self as Protobuf>::as_object(self)
    }
}

impl<T: Protobuf> ProtobufMut<'static> for T {
    fn as_object_mut(&mut self) -> &mut base::Object {
        <Self as Protobuf>::as_object_mut(self)
    }
}

pub mod tests {
    use crate::{Protobuf, ProtobufRef, ProtobufMut};

    pub fn assert_roundtrip<T: Protobuf>(msg: &T) {
        let data = msg.encode_vec::<32>().expect("msg should encode");

        let mut arena = crate::arena::Arena::new(&std::alloc::Global);
        let mut roundtrip_msg = T::default();
        assert!(roundtrip_msg.decode_flat::<32>(&mut arena, &data));

        println!(
            "Encoded {} ({} bytes)",
            T::table().descriptor.name(),
            data.len()
        );
        // println!("Roundtrip message: {:#?}", roundtrip_msg);

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

        assert!(file_descriptor.name().ends_with("descriptor.proto"));
        println!("File descriptor name: {}", file_descriptor.name());
        assert_eq!(message_descriptor.name(), "DescriptorProto");
        assert_eq!(nested_descriptor.name(), "ExtensionRange");
    }

    #[test]
    fn file_descriptor_roundtrip() {
        assert_roundtrip(
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor(),
        );
    }

    #[test]
    fn dynamic_file_descriptor_roundtrip() {
        let mut pool = crate::reflection::DescriptorPool::new(&std::alloc::Global);
        let file_descriptor =
            crate::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        pool.add_file(&file_descriptor);

        let bytes = file_descriptor.encode_vec::<32>().expect("should encode");
        let mut arena = crate::arena::Arena::new(&std::alloc::Global);
        let dynamic_file_descriptor = pool
            .decode_message("google.protobuf.FileDescriptorProto", &bytes, &mut arena)
            .expect("should decode");

        let roundtrip = dynamic_file_descriptor
            .encode_vec::<32>()
            .expect("should encode");

        assert_eq!(bytes, roundtrip);
    }
}
