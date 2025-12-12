use std::mem::MaybeUninit;

use serde::ser::SerializeStruct;

use crate::{Protobuf, encoding};
use crate::arena::Arena;
use crate::base::Object;

pub struct SerdeProtobuf<'a, T: Protobuf>(&'a T);

impl<'a, T: Protobuf> serde::Serialize for SerdeProtobuf<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let object = self.0.as_object();
        let table = T::encoding_table();
        serde_serialize(object, table, serializer)
    }
}

fn serde_serialize<S>(value: &Object, table: &encoding::Table, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut struct_serializer = serializer.serialize_struct("Protobuf", 0)?;
    let table = T::encoding_table();
    for entry in table {
        let tag = entry.tag;
        let field_name = format!("field_{}", tag >> 3);
        match entry.kind {
            crate::wire::FieldKind::Varint32 => {
                let val = value.get(entry.offset) as u32;
                struct_serializer.serialize_field(&field_name, &val)?;
            }
            crate::wire::FieldKind::Varint64 => {
                let val = value.get(entry.offset) as u64;
                struct_serializer.serialize_field(&field_name, &val)?;
            }
            crate::wire::FieldKind::Fixed32 => {
                let val = value.get(entry.offset) as u32;
                struct_serializer.serialize_field(&field_name, &val)?;
            }
            crate::wire::FieldKind::Fixed64 => {
                let val = value.get(entry.offset) as u64;
                struct_serializer.serialize_field(&field_name, &val)?;
            }
            crate::wire::FieldKind::Bytes  => {
                let val = value.get(entry.offset) as &[u8];
                struct_serializer.serialize_field(&field_name, &val)?;
            }
            crate::wire::FieldKind::Message => {
                let aux_table = table.aux_tables.get(&tag).ok_or_else(|| serde::ser::Error::custom("missing aux table"))?;
                let val = value.get::<*const Object>(entry.offset);

                let nested_serialized = serde_serialize(val, nested_table, serializer)?;
                struct_serializer.serialize_field(&field_name, &nested_serialized)?;
            }
            _ => {
                // Handle other field kinds if necessary
            }
        }
        // Serialize each field based on its type and value
        // This is a placeholder; actual implementation depends on field types
        // and how they map to serde serialization.
    }
    struct_serializer.end()

}

/* 
impl<'de, T: Protobuf> serde::DeserializeSeed<'de> for Arena<'de> {
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<T, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ProtobufVisitor<T: Protobuf> {
            marker: std::marker::PhantomData<T>,
        }

        impl<'de, T: Protobuf> serde::de::Visitor<'de> for ProtobufVisitor<T> {
            type Value = T;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a protobuf encoded byte array")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<T, E>
            where
                E: serde::de::Error,
            {
                let mut message = T::default();
                let mut arena = crate::arena::Arena::new();
                message
                    .decode_flat::<32>(&mut arena, v)
                    .then_some(message)
                    .ok_or_else(|| E::custom("failed to decode protobuf message"))
            }
        }

        deserializer.deserialize_bytes(ProtobufVisitor {
            marker: std::marker::PhantomData,
        })
    }
}
    */