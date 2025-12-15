use serde::ser::SerializeStruct;

use crate::base::{Message, Object};
use crate::google::protobuf::DescriptorProto;
use crate::google::protobuf::FieldDescriptorProto::{Label, Type};
use crate::wire;
use crate::{Protobuf, encoding};

pub struct SerdeProtobuf<'a, T: Protobuf>(pub &'a T);

impl<'a, T: Protobuf> serde::Serialize for SerdeProtobuf<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let object = self.0.as_object();
        let table = T::encoding_table();
        let descriptor = T::descriptor_proto();
        serde_serialize(object, table, descriptor, serializer)
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
                    struct_serializer.serialize_field(field_name, &[] as &[i32])?;
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
                    struct_serializer.serialize_field(field_name, &None::<i32>)?;
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
