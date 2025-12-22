use serde::ser::{SerializeSeq, SerializeStruct};

use crate::Protobuf;
use crate::base::Object;
use crate::google::protobuf::FieldDescriptorProto::{Label, Type};
use crate::reflection::{DynamicMessage, DynamicMessageArray, Value};
use crate::tables::{AuxTableEntry, Table};

impl<'msg> serde::Serialize for DynamicMessage<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let descriptor = self.descriptor();
        let mut struct_serializer = serializer.serialize_struct(descriptor.name(), descriptor.field().len())?;
        for &field in descriptor.field() {
            let Some(v) = self.get_field(field) else {
                struct_serializer.skip_field(field.name())?;
                continue;
            };
            struct_serializer.serialize_field(field.name(), &v)?;
        }
        struct_serializer.end()
    }
}

impl<'msg> serde::Serialize for DynamicMessageArray<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq_serializer = serializer.serialize_seq(Some(self.object.len()))?;
        for index in 0..self.object.len() {
            seq_serializer.serialize_element(&self.get(index))?;
        }
        seq_serializer.end()
    }
}

impl<'msg> serde::Serialize for Value<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Value::Bool(v) => serializer.serialize_bool(*v),
            Value::Int32(v) => serializer.serialize_i32(*v),
            Value::Int64(v) => serializer.serialize_i64(*v),
            Value::UInt32(v) => serializer.serialize_u32(*v),
            Value::UInt64(v) => serializer.serialize_u64(*v),
            Value::Float(v) => serializer.serialize_f32(*v),
            Value::Double(v) => serializer.serialize_f64(*v),
            Value::String(v) => serializer.serialize_str(v.as_ref()),
            Value::Bytes(v) => serializer.serialize_bytes(v.as_ref()),
            Value::Message(msg) => msg.serialize(serializer),
            Value::RepeatedBool(list) => list.serialize(serializer),
            Value::RepeatedInt32(list) => list.serialize(serializer),
            Value::RepeatedInt64(list) => list.serialize(serializer),
            Value::RepeatedUInt32(list) => list.serialize(serializer),
            Value::RepeatedUInt64(list) => list.serialize(serializer),
            Value::RepeatedFloat(list) => list.serialize(serializer),
            Value::RepeatedDouble(list) => list.serialize(serializer),
            Value::RepeatedString(list) => list.serialize(serializer),
            Value::RepeatedBytes(list) => list.serialize(serializer),
            Value::RepeatedMessage(list) => list.serialize(serializer),
        }
    }
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

pub struct SerdeDeserialize<'arena, 'alloc, T>(
    &'arena mut crate::arena::Arena<'alloc>,
    core::marker::PhantomData<T>,
);

impl<'arena, 'alloc, T> SerdeDeserialize<'arena, 'alloc, T> {
    pub fn new(arena: &'arena mut crate::arena::Arena<'alloc>) -> Self {
        SerdeDeserialize(arena, core::marker::PhantomData)
    }
}

impl<'de, 'arena, 'alloc, T: Protobuf + 'alloc> serde::de::DeserializeSeed<'de>
    for SerdeDeserialize<'arena, 'alloc, T>
{
    type Value = T;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialization logic to be implemented
        let SerdeDeserialize(arena, _) = self;
        let mut msg = T::default();
        serde_deserialize_struct(msg.as_object_mut(), T::table(), arena, deserializer)?;
        Ok(msg)
    }
}

pub struct ProtobufVisitor<'arena, 'alloc, 'b> {
    obj: &'b mut Object,
    table: &'static Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b> serde::de::DeserializeSeed<'de>
    for ProtobufVisitor<'arena, 'alloc, 'b>
{
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

fn serde_deserialize_struct<'arena, 'alloc, 'b, 'de, D>(
    obj: &'b mut Object,
    table: &'static Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
    deserializer: D,
) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    let visitor = ProtobufVisitor { obj, table, arena };
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
        self.0
            .get(v)
            .copied()
            .ok_or_else(|| serde::de::Error::unknown_field(v, &[]))
    }
}

struct ProtobufArrayfVisitor<'arena, 'alloc, 'b> {
    rf: &'b mut crate::containers::RepeatedField<crate::base::Message>,
    table: &'static Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b> serde::de::DeserializeSeed<'de>
    for ProtobufArrayfVisitor<'arena, 'alloc, 'b>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, 'arena, 'alloc, 'b> serde::de::Visitor<'de>
    for ProtobufArrayfVisitor<'arena, 'alloc, 'b>
{
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str(&format!("an array of {}", self.table.descriptor.name()))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let ProtobufArrayfVisitor { rf, table, arena } = self;
        // Loop while there are elements
        loop {
            // Create object for next element
            let mut msg_obj = Object::create(table.size as u32, arena);

            let seed = ProtobufVisitor {
                obj: &mut msg_obj,
                table,
                arena,
            };

            // Try to get next element
            match seq.next_element_seed(seed)? {
                Some(()) => {
                    // Got an element - push it
                    rf.push(crate::base::Message(msg_obj as *mut Object), arena);
                }
                None => {
                    return Ok(()); // No more elements
                }
            }
        }
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
            let entry = table.entry(field.number() as u32).unwrap(); // Safe: field exists in table
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
                    Type::TYPE_SFIXED32
                    | Type::TYPE_INT32
                    | Type::TYPE_SINT32
                    | Type::TYPE_ENUM => {
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
                        let AuxTableEntry {
                            offset,
                            child_table,
                        } = table.aux_entry_decode(entry);
                        let child_table = unsafe { &*child_table };
                        let rf = obj
                            .ref_mut::<crate::containers::RepeatedField<crate::base::Message>>(
                                offset,
                            );
                        let seed = ProtobufArrayfVisitor {
                            rf,
                            table: child_table,
                            arena,
                        };
                        map.next_value_seed(seed)?;
                    }
                },
                _ => match field.r#type().unwrap() {
                    Type::TYPE_BOOL => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<bool>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<u64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<u32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<i64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED32
                    | Type::TYPE_INT32
                    | Type::TYPE_SINT32
                    | Type::TYPE_ENUM => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<i32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FLOAT => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<f32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_DOUBLE => {
                        let Some(v) = map.next_value()? else {
                            continue;
                        };
                        obj.set::<f64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_STRING => {
                        let Some(v) = map.next_value::<Option<String>>()? else {
                            continue;
                        };
                        let s = crate::containers::String::from_str(&v, arena);
                        obj.set::<crate::containers::String>(
                            entry.offset(),
                            entry.has_bit_idx(),
                            s,
                        );
                    }
                    Type::TYPE_BYTES => {
                        let Some(v) = map.next_value::<Option<Vec<u8>>>()? else {
                            continue;
                        };
                        let b = crate::containers::Bytes::from_slice(&v, arena);
                        obj.set::<crate::containers::Bytes>(entry.offset(), entry.has_bit_idx(), b);
                    }
                    Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                        // TODO handle null
                        let AuxTableEntry {
                            offset,
                            child_table,
                        } = table.aux_entry_decode(entry);
                        let child_table = unsafe { &*child_table };
                        let child_obj = Object::create(child_table.size as u32, arena);
                        obj.set::<crate::base::Message>(
                            offset,
                            entry.has_bit_idx(),
                            crate::base::Message(child_obj),
                        );
                        let seed = ProtobufVisitor {
                            obj: child_obj,
                            table: child_table,
                            arena,
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
