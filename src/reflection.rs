use std::alloc::Allocator;

use crate::{
    Protobuf,
    arena::Arena,
    base::{Message, Object},
    containers::{Bytes, String, RepeatedField},
    google::protobuf::{
        DescriptorProto::ProtoType as DescriptorProto,
        FieldDescriptorProto::{Label, ProtoType as FieldDescriptorProto, Type},
        FileDescriptorProto::ProtoType as FileDescriptorProto,
    },
    tables::Table,
    wire,
};

pub fn field_kind_tokens(field: &&FieldDescriptorProto) -> wire::FieldKind {
    if field.label().unwrap() == Label::LABEL_REPEATED {
        match field.r#type().unwrap() {
            Type::TYPE_INT32 | Type::TYPE_UINT32 => wire::FieldKind::RepeatedVarint32,
            Type::TYPE_INT64 | Type::TYPE_UINT64 => wire::FieldKind::RepeatedVarint64,
            Type::TYPE_SINT32 => wire::FieldKind::RepeatedVarint32Zigzag,
            Type::TYPE_SINT64 => wire::FieldKind::RepeatedVarint64Zigzag,
            Type::TYPE_FIXED32 | Type::TYPE_SFIXED32 | Type::TYPE_FLOAT => {
                wire::FieldKind::RepeatedFixed32
            }
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
                wire::FieldKind::RepeatedFixed64
            }
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
            Type::TYPE_FIXED64 | Type::TYPE_SFIXED64 | Type::TYPE_DOUBLE => {
                wire::FieldKind::Fixed64
            }
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
    matches!(
        field.r#type().unwrap(),
        Type::TYPE_MESSAGE | Type::TYPE_GROUP
    )
}

pub fn needs_has_bit(field: &FieldDescriptorProto) -> bool {
    return !is_repeated(field) && !is_message(field);
}

pub fn debug_message<T: Protobuf>(msg: &T, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let dynamic_msg = crate::reflection::DynamicMessage::new(msg);
    use core::fmt::Debug;
    dynamic_msg.fmt(f)
}

/*
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
        self.files
            .push(Message(ptr as *mut Object), &mut self.arena);
    }
}
*/

pub struct DynamicMessage<'pool, 'msg> {
    pub object: &'msg Object,
    pub table: &'pool Table,
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessage<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug_struct = f.debug_struct(self.table.descriptor.name());
        for field in self.table.descriptor.field() {
            if let Some(value) = self.get_field(field) {
                debug_struct.field(field.name(), &value);
            }
        }
        debug_struct.finish()
    }
}

impl<'pool, 'msg> DynamicMessage<'pool, 'msg> {
    pub fn new<T>(msg: &'msg T) -> Self
    where
        T: crate::Protobuf,
    {
        DynamicMessage {
            object: msg.as_object(),
            table: T::table(),
        }
    }

    pub fn table(&self) -> &Table {
        self.table
    }

    pub fn descriptor(&self) -> &DescriptorProto {
        self.table.descriptor
    }

    pub fn find_field_descriptor(&self, field_name: &str) -> Option<&'pool FieldDescriptorProto> {
        for field in self.table.descriptor.field().iter() {
            if field.name() == field_name {
                return Some(field);
            }
        }
        None
    }

    pub fn get_field(&'msg self, field: &'pool FieldDescriptorProto) -> Option<Value<'pool, 'msg>> {
        let entry = self.table.entry(field.number() as u32).unwrap();
        if field.label().unwrap() == Label::LABEL_REPEATED {
            // Repeated field
            match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    let slice = self.object.get_slice::<i32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedInt32(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let slice = self.object.get_slice::<i64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedInt64(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let slice = self.object.get_slice::<u32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedUInt32(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let slice = self.object.get_slice::<u64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedUInt64(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_FLOAT => {
                    let slice = self.object.get_slice::<f32>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedFloat(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_DOUBLE => {
                    let slice = self.object.get_slice::<f64>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedDouble(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_BOOL => {
                    let slice = self.object.get_slice::<bool>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedBool(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_STRING => {
                    let slice = self.object.get_slice::<String>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedString(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_BYTES => {
                    let slice = self.object.get_slice::<Bytes>(entry.offset() as usize);
                    if !slice.is_empty() {
                        Some(Value::RepeatedBytes(slice))
                    } else {
                        None
                    }
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let aux = self.table.aux_entry_decode(entry);
                    let slice = self.object.get_slice::<Message>(aux.offset as usize);
                    if slice.is_empty() {
                        return None;
                    }
                    let dynamic_array = DynamicMessageArray {
                        object: slice,
                        table: unsafe { &*aux.child_table },
                    };
                    Some(Value::RepeatedMessage(dynamic_array))
                }
            }
        } else {
            let value = match field.r#type().unwrap() {
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    Value::Int32(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    Value::Int64(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    Value::UInt32(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    Value::UInt64(self.object.get(entry.offset() as usize))
                }
                Type::TYPE_FLOAT => Value::Float(self.object.get(entry.offset() as usize)),
                Type::TYPE_DOUBLE => Value::Double(self.object.get(entry.offset() as usize)),
                Type::TYPE_BOOL => Value::Bool(self.object.get(entry.offset() as usize)),
                Type::TYPE_STRING => Value::String(
                    self.object
                        .ref_at::<crate::containers::String>(entry.offset() as usize)
                        .as_str(),
                ),
                Type::TYPE_BYTES => {
                    Value::Bytes(self.object.get_slice::<u8>(entry.offset() as usize))
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let aux_entry = self.table.aux_entry_decode(entry);
                    let offset = aux_entry.offset as usize;
                    let msg = self.object.get::<Message>(offset);
                    if msg.0.is_null() {
                        return None;
                    }
                    let dynamic_msg = DynamicMessage {
                        object: unsafe { &mut *msg.0 },
                        table: unsafe { &*aux_entry.child_table },
                    };
                    return Some(Value::Message(dynamic_msg));
                }
            };
            debug_assert!(needs_has_bit(field));
            if self.object.has_bit(entry.has_bit_idx() as u8) {
                Some(value)
            } else {
                None
            }
        }
    }
}

pub struct DynamicMessageArray<'pool, 'msg> {
    pub object: &'msg [Message],
    pub table: &'pool Table,
}

impl<'pool, 'msg> core::fmt::Debug for DynamicMessageArray<'pool, 'msg> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.object.iter().map(|msg| DynamicMessage {
                object: unsafe { &mut *msg.0 },
                table: self.table,
            }))
            .finish()
    }
}

impl<'pool, 'msg> DynamicMessageArray<'pool, 'msg> {
    pub fn len(&self) -> usize {
        self.object.len()
    }

    pub fn is_empty(&self) -> bool {
        self.object.is_empty()
    }

    pub fn get(&self, index: usize) -> DynamicMessage<'pool, 'msg> {
        let obj = self.object[index];
        DynamicMessage {
            object: unsafe { &mut *obj.0 },
            table: self.table,
        }
    }

    pub fn iter<'a>(&'a self) -> DynamicMessageArrayIter<'pool, 'a> {
        DynamicMessageArrayIter {
            array: self,
            index: 0,
        }
    }
}

// Iterator struct
pub struct DynamicMessageArrayIter<'pool, 'msg> {
    array: &'msg DynamicMessageArray<'pool, 'msg>,
    index: usize,
}

impl<'pool, 'msg> Iterator for DynamicMessageArrayIter<'pool, 'msg> {
    type Item = DynamicMessage<'pool, 'msg>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.array.len() {
            let item = self.array.get(self.index);
            self.index += 1;
            Some(item)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.array.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<'pool, 'msg> ExactSizeIterator for DynamicMessageArrayIter<'pool, 'msg> {
    fn len(&self) -> usize {
        self.array.len() - self.index
    }
}

// IntoIterator for easy use with for loops
impl<'pool, 'msg> IntoIterator for &'msg DynamicMessageArray<'pool, 'msg> {
    type Item = DynamicMessage<'pool, 'msg>;
    type IntoIter = DynamicMessageArrayIter<'pool, 'msg>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug)]
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
