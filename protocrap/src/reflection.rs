use std::alloc::Allocator;

use crate::{
    arena::Arena, base::{Message, Object}, containers::{Bytes, RepeatedField}, google::protobuf::{FieldDescriptorProto::{
        Label, ProtoType as FieldDescriptorProto, Type
    }, FileDescriptorProto::ProtoType as FileDescriptorProto}, wire
};

pub fn field_kind_tokens(field: &&FieldDescriptorProto) -> wire::FieldKind {
    if field.label().unwrap() == Label::LABEL_REPEATED {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 | Type::TYPE_UINT32 => wire::FieldKind::RepeatedVarint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::RepeatedVarint64,
            Type::TYPE_SINT32 => wire::FieldKind::RepeatedVarint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::RepeatedVarint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => wire::FieldKind::RepeatedFixed32,
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => wire::FieldKind::RepeatedFixed64,
            Type::TYPE_BOOL => wire::FieldKind::RepeatedBool,
            Type::TYPE_STRING | Type::TYPE_BYTES => wire::FieldKind::RepeatedBytes,
            Type::TYPE_MESSAGE => wire::FieldKind::RepeatedMessage,
            Type::TYPE_GROUP => wire::FieldKind::RepeatedGroup,
            Type::TYPE_ENUM => wire::FieldKind::RepeatedVarint32,
        }
    } else {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 | Type::TYPE_UINT32 => wire::FieldKind::Varint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::Varint64,
            Type::TYPE_SINT32 => wire::FieldKind::Varint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::Varint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => wire::FieldKind::Fixed32,
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => wire::FieldKind::Fixed64,
            Type::TYPE_BOOL => wire::FieldKind::Bool,
            Type::TYPE_STRING | Type::TYPE_BYTES => wire::FieldKind::Bytes,
            Type::TYPE_MESSAGE => wire::FieldKind::Message,
            Type::TYPE_GROUP => wire::FieldKind::Group,
            Type::TYPE_ENUM => wire::FieldKind::Varint32,
        }
    }
}

pub fn calculate_tag(field: &FieldDescriptorProto) -> u32 {
    let wire_type = match field.r#type().unwrap() {
        Type::TYPE_INT32
        | Type::TYPE_INT64
        | Type::TYPE_UINT32
        | Type::TYPE_UINT64
        | Type::TYPE_SINT32
        | Type::TYPE_SINT64
        | Type::TYPE_BOOL
        | Type::TYPE_ENUM => 0,
        Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => 1,
        Type::TYPE_STRING | Type::TYPE_BYTES | Type::TYPE_MESSAGE => 2,
        Type::TYPE_GROUP => 3,
        Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => 5,
    };

    (field.number() as u32) << 3 | wire_type
}

pub fn is_repeated(field: &FieldDescriptorProto) -> bool {
    field.label().unwrap() == Label::LABEL_REPEATED
}

pub fn is_message(field: &FieldDescriptorProto) -> bool {
    matches!(field.r#type().unwrap(), Type::TYPE_MESSAGE | Type::TYPE_GROUP)
}

pub fn needs_has_bit(field: &FieldDescriptorProto) -> bool {
    return !is_repeated(field) && !is_message(field);
}


struct DescriptorPool {
    pub arena: Arena<'static>,

    // 
    files: RepeatedField<Message>,
    //map: std::collections::HashMap<String, &'static FileDescriptorProto>,

}

impl DescriptorPool {
    pub fn new(alloc: &'static dyn Allocator) -> Self {
        DescriptorPool {
            arena: Arena::new(alloc),
            files: RepeatedField::new(),
        }
    }

    pub fn add_file(&mut self, file: FileDescriptorProto) {
        let ptr = self.arena.alloc::<FileDescriptorProto>();
        unsafe {
            core::ptr::write(ptr, file);
        }
        self.files.push(Message(ptr as *mut Object), &mut self.arena);
    }


}

pub struct DynamicMessage<'pool, 'msg> {
    pub object: &'msg mut Object,
    pub encoding_table: &'pool [crate::encoding::TableEntry],
    pub decoding_table: &'pool crate::decoding::Table,
    pub descriptor: &'pool FileDescriptorProto,
}

impl<'pool, 'msg> DynamicMessage<'pool, 'msg> {
    pub fn find_field_descriptor(
        &self,
        field_name: &str,
    ) -> Option<&'pool FieldDescriptorProto> {
        for field in self.descriptor.message_type().iter().flat_map(|m| m.field().iter()) {
            if field.name() == field_name {
                return Some(field);
            }
        }
        None
    }

    pub fn get_field(&'msg self, field: &'pool FieldDescriptorProto) -> Option<Value<'pool, 'msg>> {
        let entry = self.decoding_table.entry(field.number() as u32).unwrap();
        if field.label().unwrap() == Label::LABEL_REPEATED {
            // Repeated field
            match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    Some(Value::RepeatedInt32(self.object.get_slice::<i32>(entry.offset() as usize)))
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    Some(Value::RepeatedInt64(self.object.get_slice::<i64>(entry.offset() as usize)))
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    Some(Value::RepeatedUInt32(self.object.get_slice::<u32>(entry.offset() as usize)))
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    Some(Value::RepeatedUInt64(self.object.get_slice::<u64>(entry.offset() as usize)))
                }
                Type::TYPE_FLOAT => {
                    Some(Value::RepeatedFloat(self.object.get_slice::<f32>(entry.offset() as usize)))
                }
                Type::TYPE_DOUBLE => {
                    Some(Value::RepeatedDouble(self.object.get_slice::<f64>(entry.offset() as usize)))
                }
                Type::TYPE_BOOL => {
                    Some(Value::RepeatedBool(self.object.get_slice::<bool>(entry.offset() as usize)))
                }
                Type::TYPE_STRING => {
                    Some(Value::RepeatedString(self.object.get_slice::<String>(entry.offset() as usize)))
                }
                Type::TYPE_BYTES => {
                    Some(Value::RepeatedBytes(self.object.get_slice::<Bytes>(entry.offset() as usize)))
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let aux = self.decoding_table.aux_entry(entry);
                    let slice = self.object.get_slice::<Message>(aux.offset as usize);
                    let dynamic_array = DynamicMessageArray {
                        object: slice,
                        encoding_table: self.encoding_table,
                        decoding_table: self.decoding_table,
                        descriptor: self.descriptor,
                    };
                    Some(Value::RepeatedMessage(dynamic_array))
                }
            }
        } else {
            match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    Some(Value::Int32(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    Some(Value::Int64(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    Some(Value::UInt32(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    Some(Value::UInt64(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_FLOAT => {
                    Some(Value::Float(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_DOUBLE => {
                    Some(Value::Double(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_BOOL => {
                    Some(Value::Bool(self.object.get(entry.offset() as usize)))
                }
                Type::TYPE_STRING => {
                    Some(Value::String(self.object.ref_at::<String>(entry.offset() as usize).as_str()))
                }
                Type::TYPE_BYTES => {
                    Some(Value::Bytes(self.object.get_slice::<u8>(entry.offset() as usize)))
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let aux_entry = self.decoding_table.aux_entry(entry);
                    let offset = aux_entry.offset as usize;
                    let msg = self.object.get::<Message>(offset);
                    let dynamic_msg = DynamicMessage {
                        object: unsafe { &mut *msg.0 },
                        encoding_table: self.encoding_table,
                        decoding_table: self.decoding_table,
                        descriptor: self.descriptor,
                    };
                    Some(Value::Message(dynamic_msg))
                }
            }
        }
    }
}

pub struct DynamicMessageArray<'pool, 'msg> {
    pub object: &'msg [Message],
    pub encoding_table: &'pool [crate::encoding::TableEntry],
    pub decoding_table: &'pool crate::decoding::Table,
    pub descriptor: &'pool FileDescriptorProto,
}

impl<'pool, 'msg> DynamicMessageArray<'pool, 'msg> {
    pub fn len(&self) -> usize {
        self.object.len()
    }

    pub fn get(&self, index: usize) -> DynamicMessage<'pool, 'msg> {
        let obj = self.object[index];
        DynamicMessage {
            object: unsafe { &mut *obj.0 },
            encoding_table: self.encoding_table,
            decoding_table: self.decoding_table,
            descriptor: self.descriptor,
        }
    }
}

pub enum Value<'pool, 'msg> {
    Int32(i32),
    Int64(i64),
    UInt32(u32),
    UInt64(u64),
    Float(f32),
    Double(f64),
    Bool(bool),
    String(&'msg str),
    Bytes(&'msg [u8]),
    Message(DynamicMessage<'pool, 'msg>),
    RepeatedInt32(&'msg [i32]),
    RepeatedInt64(&'msg [i64]),
    RepeatedUInt32(&'msg [u32]),
    RepeatedUInt64(&'msg [u64]),
    RepeatedFloat(&'msg [f32]),
    RepeatedDouble(&'msg [f64]),
    RepeatedBool(&'msg [bool]),
    RepeatedString(&'msg [String]),
    RepeatedBytes(&'msg [Bytes]),
    RepeatedMessage(DynamicMessageArray<'pool, 'msg>),
}
