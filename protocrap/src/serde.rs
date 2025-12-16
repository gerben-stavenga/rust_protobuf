use serde::de;
use serde::ser::{SerializeSeq, SerializeStruct};

use crate::base::{Message, Object};
use crate::decoding::AuxTableEntry;
use crate::encoding::aux_entry;
use crate::google::protobuf::DescriptorProto;
use crate::google::protobuf::FieldDescriptorProto::{Label, Type};
use crate::{Protobuf, encoding, decoding};

pub struct SerdeProtobuf<'a>(
    pub &'a Object,
    pub &'static [encoding::TableEntry],
    pub &'static DescriptorProto::ProtoType,
);

pub struct SerdeProtobufSlice<'a>(
    pub &'a [Message],
    pub &'static [encoding::TableEntry],
    pub &'static DescriptorProto::ProtoType,
);

impl<'a> SerdeProtobuf<'a> {
    pub fn new<T: Protobuf>(msg: &'a T) -> Self {
        SerdeProtobuf(msg.as_object(), T::encoding_table(), T::descriptor_proto())
    }
}

impl<'a> serde::Serialize for SerdeProtobuf<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde_serialize(self.0, self.1, self.2, serializer)
    }
}

impl<'a> serde::Serialize for SerdeProtobufSlice<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq_serializer = serializer.serialize_seq(Some(self.0.len()))?;
        for msg in self.0 {
            let serde_msg = SerdeProtobuf(unsafe { &*msg.0 }, self.1, self.2);
            seq_serializer.serialize_element(&serde_msg)?;
        }
        seq_serializer.end()
    }
}

fn calculate_tag(field: &crate::google::protobuf::FieldDescriptorProto::ProtoType) -> u32 {
    let number = field.number() as u32;
    let wire_type = match field.r#type().unwrap() {
        crate::google::protobuf::FieldDescriptorProto::Type::TYPE_DOUBLE
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_FIXED64
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_SFIXED64 => 1,
        crate::google::protobuf::FieldDescriptorProto::Type::TYPE_FLOAT
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_FIXED32
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_SFIXED32 => 5,
        crate::google::protobuf::FieldDescriptorProto::Type::TYPE_INT64
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_UINT64
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_INT32
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_UINT32
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_BOOL
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_ENUM
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_SINT32
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_SINT64 => 0,
        crate::google::protobuf::FieldDescriptorProto::Type::TYPE_STRING
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_BYTES
        | crate::google::protobuf::FieldDescriptorProto::Type::TYPE_MESSAGE => 2,
        crate::google::protobuf::FieldDescriptorProto::Type::TYPE_GROUP => 3,
    };
    (number << 3) | wire_type
}

fn serde_serialize<S>(
    value: &Object,
    table: &[crate::encoding::TableEntry],
    descriptor: &'static DescriptorProto::ProtoType,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let mut struct_serializer = serializer.serialize_struct(descriptor.name(), table.len())?;
    for entry in table {
        let tag = entry.encoded_tag;
        let field = *descriptor
            .field()
            .iter()
            .find(|f| calculate_tag(f) == tag)
            .ok_or_else(|| serde::ser::Error::custom("missing field descriptor"))?;
        let field_name = field.name();
        match field.label().unwrap() {
            Label::LABEL_REPEATED => match field.r#type().unwrap() {
                Type::TYPE_BOOL => {
                    let slice = value.get_slice::<bool>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                    let slice = value.get_slice::<u64>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                    let slice = value.get_slice::<u32>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                    let slice = value.get_slice::<i64>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_SFIXED32 | Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_ENUM => {
                    let slice = value.get_slice::<i32>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_FLOAT => {
                    let slice = value.get_slice::<f32>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_DOUBLE => {
                    let slice = value.get_slice::<f64>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_STRING => {
                    let slice = value.get_slice::<crate::containers::String>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_BYTES => {
                    let slice = value.get_slice::<crate::containers::Bytes>(entry.offset as usize);
                    struct_serializer.serialize_field(field_name, slice)?;
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let (offset, child_table) = aux_entry(entry.offset as usize, table);
                    let slice = value.get_slice::<crate::base::Message>(offset);
                    let serde_slice = SerdeProtobufSlice(slice, child_table, unsafe {
                        *(child_table.as_ptr() as *const &'static DescriptorProto::ProtoType).sub(1)
                    });
                    struct_serializer.serialize_field(field_name, &serde_slice)?;
                    continue;
                }
            },
            _ => match field.r#type().unwrap() {
                Type::TYPE_BOOL => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<bool>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<u64>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<u32>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<i64>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_SFIXED32 | Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_ENUM => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<i32>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_FLOAT => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<f32>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_DOUBLE => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(value.get::<f64>(entry.offset as usize))
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_STRING => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(
                            value
                                .ref_at::<crate::containers::String>(entry.offset as usize)
                                .as_str(),
                        )
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_BYTES => {
                    let v = if value.has_bit(entry.has_bit) {
                        Some(
                            value
                                .ref_at::<crate::containers::Bytes>(entry.offset as usize)
                                .slice(),
                        )
                    } else {
                        None
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let (offset, child_table) = aux_entry(entry.offset as usize, table);
                    let message = value.get::<crate::base::Message>(offset).0;
                    let v = if message.is_null() {
                        None
                    } else {
                        let v = SerdeProtobuf(unsafe { &*message }, child_table, unsafe {
                            *(child_table.as_ptr() as *const &'static DescriptorProto::ProtoType)
                                .sub(1)
                        });
                        Some(v)
                    };
                    struct_serializer.serialize_field(field_name, &v)?;
                }
            },
        }
    }
    struct_serializer.end()
}

impl serde::Serialize for crate::containers::Bytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.as_ref())
    }
}

impl serde::Serialize for crate::containers::String {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }
}

pub struct SerdeDeserialize<'arena, 'alloc, T>(&'arena mut crate::arena::Arena<'alloc>, core::marker::PhantomData<T>);

impl<'arena, 'alloc, T> SerdeDeserialize<'arena, 'alloc, T> {
    pub fn new(arena: &'arena mut crate::arena::Arena<'alloc>) -> Self {
        SerdeDeserialize(arena, core::marker::PhantomData)
    }
}

impl<'de, 'arena, 'alloc, T: Protobuf + 'alloc> serde::de::DeserializeSeed<'de> for SerdeDeserialize<'arena, 'alloc, T> {
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialization logic to be implemented
        let SerdeDeserialize(arena, _) = self;
        let mut msg = T::default();
        serde_deserialize_struct(msg.as_object_mut(), T::decoding_table(), arena, deserializer)?;
        Ok(msg)
    }
}

pub struct ProtobufVisitor<'arena, 'alloc, 'b> {
    obj: &'b mut Object,
    table: &'static decoding::Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b> serde::de::DeserializeSeed<'de> for ProtobufVisitor<'arena, 'alloc, 'b> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialization logic to be implemented
        let ProtobufVisitor { obj, table, arena } = self;
        serde_deserialize_struct(obj, table, arena, deserializer)?;
        Ok(())
    }
}

fn serde_deserialize_struct<'arena, 'alloc, 'b, 'de, D>(obj: &'b mut Object, table: &'static decoding::Table, arena: &'arena mut crate::arena::Arena<'alloc>, deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    let visitor = ProtobufVisitor {
        obj,
        table,
        arena,
    };
    let fields = table.descriptor.field();
    let field_names: Vec<&str> = fields.iter().map(|f| f.name()).collect();
    let field_names_slice = field_names.as_slice();
    let field_names_static = unsafe { std::mem::transmute(field_names_slice) };
    deserializer.deserialize_struct(table.descriptor.name(), field_names_static, visitor)
}

struct StructKeyVisitor<'a>(&'a std::collections::HashMap<&'static str, usize>);

impl<'de> serde::de::DeserializeSeed<'de> for StructKeyVisitor<'_> {
    type Value = usize;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_identifier(self)
    }
}

impl<'de> serde::de::Visitor<'de> for StructKeyVisitor<'_> {
    type Value = usize;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("a valid field name")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.0.get(v).copied().ok_or_else(|| serde::de::Error::unknown_field(v, &[]))
    }
}

impl<'de, 'arena, 'alloc, 'b> serde::de::Visitor<'de> for ProtobufVisitor<'arena, 'alloc, 'b> {
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str(self.table.descriptor.name())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let ProtobufVisitor { obj, table, arena } = self;

        let mut field_map = std::collections::HashMap::new();
        for (field_index, field) in table.descriptor.field().iter().enumerate() {
            let field_name = field.name();
            field_map.insert(field_name, field_index);
        }
        while let Some(idx) = map.next_key_seed(StructKeyVisitor(&field_map))? {
            let field = table.descriptor.field()[idx];
            let entry = table.entry(field.number() as u32);
            match field.label().unwrap() {
                Label::LABEL_REPEATED => match field.r#type().unwrap() {
                    Type::TYPE_BOOL => {
                        let slice: Vec<bool> = map.next_value()?;
                        for v in slice {
                            obj.add::<bool>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                        let slice: Vec<u64> = map.next_value()?;
                        for v in slice {
                            obj.add::<u64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let slice: Vec<u32> = map.next_value()?;
                        for v in slice {
                            obj.add::<u32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let slice: Vec<i64> = map.next_value()?;
                        for v in slice {
                            obj.add::<i64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_SFIXED32 | Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_ENUM => {
                        let slice: Vec<i32> = map.next_value()?;
                        for v in slice {
                            obj.add::<i32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FLOAT => {
                        let slice: Vec<f32> = map.next_value()?;
                        for v in slice {
                            obj.add::<f32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_DOUBLE => {
                        let slice: Vec<f64> = map.next_value()?;
                        for v in slice {
                            obj.add::<f64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_STRING => {
                        let slice: Vec<String> = map.next_value()?;
                        for v in slice {
                            let s = crate::containers::String::from_str(&v, arena);
                            obj.add::<crate::containers::String>(entry.offset(), s, arena);
                        }
                    }
                    Type::TYPE_BYTES => {
                        let slice: Vec<Vec<u8>> = map.next_value()?;
                        for v in slice {
                            let b = crate::containers::Bytes::from_slice(&v, arena);
                            obj.add::<crate::containers::Bytes>(entry.offset(), b, arena);
                        }
                    }
                    Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                        unimplemented!()
                    },
                }
                _ => match field.r#type().unwrap() {
                    Type::TYPE_BOOL => {
                        let v: bool = map.next_value()?;
                        obj.set::<bool>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                        let v: u64 = map.next_value()?;
                        obj.set::<u64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let v: u32 = map.next_value()?;
                        obj.set::<u32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let v: i64 = map.next_value()?;
                        obj.set::<i64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED32 | Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_ENUM => {
                        let v: i32 = map.next_value()?;
                        obj.set::<i32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FLOAT => {
                        let v: f32 = map.next_value()?;
                        obj.set::<f32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_DOUBLE => {
                        let v: f64 = map.next_value()?;
                        obj.set::<f64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_STRING => {
                        let v: String = map.next_value()?;
                        let s = crate::containers::String::from_str(&v, arena);
                        obj.set::<crate::containers::String>(entry.offset(), entry.has_bit_idx(), s);
                    }
                    Type::TYPE_BYTES => {
                        let v: Vec<u8> = map.next_value()?;
                        let b = crate::containers::Bytes::from_slice(&v, arena);
                        obj.set::<crate::containers::Bytes>(entry.offset(), entry.has_bit_idx(), b);
                    }
                    Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                        let &decoding::AuxTableEntry { offset, child_table }  = table.aux_entry(field.number() as u32);
                        let child_table = unsafe {
                             &*child_table
                        };
                        let child_obj = Object::create(child_table.size as u32, arena);
                        obj.set::<crate::base::Message>(offset, entry.has_bit_idx(), crate::base::Message(child_obj));
                        let seed = ProtobufVisitor {
                            obj: child_obj,
                            table: child_table,
                            arena, // FIXME: this is ugly but works around lifetime issues
                        };
                        map.next_value_seed(seed)?;
                    }   
                },
            }
            // Process each field in the map
        }
        Ok(())
    }

}