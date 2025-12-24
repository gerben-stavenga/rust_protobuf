use serde::ser::{SerializeSeq, SerializeStruct};

use crate::Protobuf;
use crate::base::Object;
use crate::google::protobuf::FieldDescriptorProto::{Label, Type};
use crate::reflection::{DynamicMessage, DynamicMessageArray, Value, default_value};
use crate::tables::{AuxTableEntry, Table};

// Well-known type detection and handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WellKnownType {
    BoolValue,
    Int32Value,
    Int64Value,
    UInt32Value,
    UInt64Value,
    FloatValue,
    DoubleValue,
    StringValue,
    BytesValue,
    Timestamp,
    Duration,
    None,
}

// TODO: This should check the full qualified name (package.Type) instead of just the type name.
// Currently fragile - will incorrectly match user-defined types with the same name.
// Proper implementation would check descriptor's package and construct full name.
fn detect_well_known_type(descriptor: &crate::google::protobuf::DescriptorProto::ProtoType) -> WellKnownType {
    match descriptor.name() {
        "BoolValue" => WellKnownType::BoolValue,
        "Int32Value" => WellKnownType::Int32Value,
        "Int64Value" => WellKnownType::Int64Value,
        "UInt32Value" => WellKnownType::UInt32Value,
        "UInt64Value" => WellKnownType::UInt64Value,
        "FloatValue" => WellKnownType::FloatValue,
        "DoubleValue" => WellKnownType::DoubleValue,
        "StringValue" => WellKnownType::StringValue,
        "BytesValue" => WellKnownType::BytesValue,
        "Timestamp" => WellKnownType::Timestamp,
        "Duration" => WellKnownType::Duration,
        _ => WellKnownType::None,
    }
}

// Timestamp validation and formatting
fn validate_timestamp(seconds: i64, nanos: i32) -> Result<(), &'static str> {
    // RFC 3339 valid range: 0001-01-01T00:00:00Z to 9999-12-31T23:59:59.999999999Z
    // Corresponds to: -62135596800 to 253402300799 seconds
    if seconds < -62135596800 || seconds > 253402300799 {
        return Err("Timestamp seconds out of valid range");
    }
    if nanos < 0 || nanos > 999_999_999 {
        return Err("Timestamp nanos must be in range [0, 999999999]");
    }
    Ok(())
}

fn format_timestamp(seconds: i64, nanos: i32) -> Result<std::string::String, &'static str> {
    validate_timestamp(seconds, nanos)?;

    let dt = time::OffsetDateTime::from_unix_timestamp(seconds)
        .map_err(|_| "Invalid timestamp")?;

    // Add nanoseconds
    let dt = dt + time::Duration::nanoseconds(nanos as i64);

    dt.format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| "Format error")
}

fn parse_timestamp(s: &str) -> Result<(i64, i32), &'static str> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|_| "Invalid RFC 3339 timestamp")?;

    let seconds = dt.unix_timestamp();
    let nanos = dt.nanosecond() as i32;

    validate_timestamp(seconds, nanos)?;
    Ok((seconds, nanos))
}

// Duration validation and formatting
fn validate_duration(seconds: i64, nanos: i32) -> Result<(), &'static str> {
    // Valid range: -315576000000 to +315576000000 seconds (approximately 10,000 years)
    if seconds < -315576000000 || seconds > 315576000000 {
        return Err("Duration seconds out of valid range");
    }
    if nanos < -999_999_999 || nanos > 999_999_999 {
        return Err("Duration nanos must be in range [-999999999, 999999999]");
    }
    // Check sign consistency
    if (seconds > 0 && nanos < 0) || (seconds < 0 && nanos > 0) {
        return Err("Duration seconds and nanos must have the same sign");
    }
    Ok(())
}

fn format_duration(seconds: i64, nanos: i32) -> Result<std::string::String, &'static str> {
    validate_duration(seconds, nanos)?;

    if seconds == 0 && nanos == 0 {
        return Ok("0s".to_string());
    }

    if nanos == 0 {
        return Ok(format!("{}s", seconds));
    }

    // Combine seconds and nanos, handle negative properly
    let total_nanos = seconds * 1_000_000_000 + nanos as i64;
    let abs_seconds = total_nanos.abs() / 1_000_000_000;
    let abs_nanos = (total_nanos.abs() % 1_000_000_000) as u32;

    let mut result = if total_nanos < 0 {
        format!("-{}.{:09}s", abs_seconds, abs_nanos)
    } else {
        format!("{}.{:09}s", abs_seconds, abs_nanos)
    };

    // Trim trailing zeros from fractional part
    result = result.trim_end_matches('0').trim_end_matches('.').to_string();
    if !result.ends_with('s') {
        result.push('s');
    }

    Ok(result)
}

fn parse_duration(s: &str) -> Result<(i64, i32), &'static str> {
    if !s.ends_with('s') {
        return Err("Duration must end with 's'");
    }

    let s = &s[..s.len() - 1]; // Remove 's' suffix

    if let Some(dot_pos) = s.find('.') {
        let (sec_str, frac_str) = s.split_at(dot_pos);
        let frac_str = &frac_str[1..]; // Skip the '.'

        let seconds: i64 = sec_str.parse().map_err(|_| "Invalid seconds")?;

        // Pad or truncate fractional part to 9 digits
        let mut frac_padded = frac_str.to_string();
        frac_padded.truncate(9);
        while frac_padded.len() < 9 {
            frac_padded.push('0');
        }

        let mut nanos: i32 = frac_padded.parse().map_err(|_| "Invalid nanos")?;

        // Apply sign from seconds to nanos
        if seconds < 0 {
            nanos = -nanos;
        }

        Ok((seconds, nanos))
    } else {
        let seconds: i64 = s.parse().map_err(|_| "Invalid seconds")?;
        Ok((seconds, 0))
    }
}

// Helper to serialize wrapper types
fn serialize_wrapper<S, T>(
    msg: &DynamicMessage,
    serializer: S,
    extract: impl FnOnce(&Value) -> Option<T>,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: serde::Serialize,
{
    // Find field number 1 (the "value" field in all wrappers)
    let field = msg.find_field_descriptor_by_number(1)
        .ok_or_else(|| serde::ser::Error::custom("Wrapper missing 'value' field"))?;

    if let Some(value) = msg.get_field(field) {
        if let Some(unwrapped) = extract(&value) {
            unwrapped.serialize(serializer)
        } else {
            Err(serde::ser::Error::custom("Wrapper value has wrong type"))
        }
    } else {
        // Null value for missing wrapper
        serializer.serialize_none()
    }
}

fn serialize_timestamp<S>(msg: &DynamicMessage, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let seconds_field = msg.find_field_descriptor_by_number(1)
        .ok_or_else(|| serde::ser::Error::custom("Timestamp missing 'seconds' field"))?;
    let nanos_field = msg.find_field_descriptor_by_number(2)
        .ok_or_else(|| serde::ser::Error::custom("Timestamp missing 'nanos' field"))?;

    let seconds = match msg.get_field(seconds_field) {
        Some(Value::Int64(s)) => s,
        _ => 0i64,
    };

    let nanos = match msg.get_field(nanos_field) {
        Some(Value::Int32(n)) => n,
        _ => 0i32,
    };

    let timestamp_str = format_timestamp(seconds, nanos)
        .map_err(serde::ser::Error::custom)?;

    serializer.serialize_str(&timestamp_str)
}

fn serialize_duration<S>(msg: &DynamicMessage, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let seconds_field = msg.find_field_descriptor_by_number(1)
        .ok_or_else(|| serde::ser::Error::custom("Duration missing 'seconds' field"))?;
    let nanos_field = msg.find_field_descriptor_by_number(2)
        .ok_or_else(|| serde::ser::Error::custom("Duration missing 'nanos' field"))?;

    let seconds = match msg.get_field(seconds_field) {
        Some(Value::Int64(s)) => s,
        _ => 0i64,
    };

    let nanos = match msg.get_field(nanos_field) {
        Some(Value::Int32(n)) => n,
        _ => 0i32,
    };

    let duration_str = format_duration(seconds, nanos)
        .map_err(serde::ser::Error::custom)?;

    serializer.serialize_str(&duration_str)
}

impl<'msg> serde::Serialize for DynamicMessage<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let descriptor = self.descriptor();

        // Check if this is a well-known type
        match detect_well_known_type(descriptor) {
            WellKnownType::BoolValue => serialize_wrapper(self, serializer, |v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            }),
            WellKnownType::Int32Value => serialize_wrapper(self, serializer, |v| match v {
                Value::Int32(i) => Some(*i),
                _ => None,
            }),
            WellKnownType::Int64Value => serialize_wrapper(self, serializer, |v| match v {
                Value::Int64(i) => Some(*i),
                _ => None,
            }),
            WellKnownType::UInt32Value => serialize_wrapper(self, serializer, |v| match v {
                Value::UInt32(u) => Some(*u),
                _ => None,
            }),
            WellKnownType::UInt64Value => serialize_wrapper(self, serializer, |v| match v {
                Value::UInt64(u) => Some(*u),
                _ => None,
            }),
            WellKnownType::FloatValue => serialize_wrapper(self, serializer, |v| match v {
                Value::Float(f) => Some(*f),
                _ => None,
            }),
            WellKnownType::DoubleValue => serialize_wrapper(self, serializer, |v| match v {
                Value::Double(d) => Some(*d),
                _ => None,
            }),
            WellKnownType::StringValue => {
                let field = self.find_field_descriptor_by_number(1)
                    .ok_or_else(|| serde::ser::Error::custom("StringValue missing 'value' field"))?;
                if let Some(Value::String(s)) = self.get_field(field) {
                    serializer.serialize_str(s)
                } else {
                    serializer.serialize_none()
                }
            }
            WellKnownType::BytesValue => {
                let field = self.find_field_descriptor_by_number(1)
                    .ok_or_else(|| serde::ser::Error::custom("BytesValue missing 'value' field"))?;
                if let Some(Value::Bytes(b)) = self.get_field(field) {
                    use serde::Serialize;
                    base64::engine::Engine::encode(&base64::engine::general_purpose::STANDARD, b).serialize(serializer)
                } else {
                    serializer.serialize_none()
                }
            }
            WellKnownType::Timestamp => serialize_timestamp(self, serializer),
            WellKnownType::Duration => serialize_duration(self, serializer),
            WellKnownType::None => {
                // Regular message serialization
                let mut fields = Vec::new();
                for &field in descriptor.field() {
                    let Some(v) = self.get_field(field) else {
                        continue;
                    };
                    fields.push((field.json_name(), v));
                }
                let mut struct_serializer = serializer.serialize_struct("", fields.len())?;
                for (name, value) in fields {
                    struct_serializer.serialize_field(name, &value)?;
                }
                struct_serializer.end()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MapKey {
    Bool(bool),
    Int32(i32),
    Int64(i64),
    UInt32(u32),
    UInt64(u64),
    String(std::string::String),
}

impl<'msg> serde::Serialize for DynamicMessageArray<'static, 'msg> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.table.descriptor.options().map(|o| o.map_entry()).unwrap_or(false) {
            use serde::ser::SerializeMap;
            let mut map_serializer = serializer.serialize_map(Some(self.object.len()))?;

            let mut seen_keys = std::collections::hash_set::HashSet::<MapKey>::new();
            for index in (0..self.object.len()).rev() {
                let entry = self.get(index);
                let key_field = entry.find_field_descriptor_by_number(1).ok_or_else(|| {
                    serde::ser::Error::custom("Map entry missing key field")
                })?;
                let value_field = entry.find_field_descriptor_by_number(2).ok_or_else(|| {
                    serde::ser::Error::custom("Map entry missing value field")
                })?;
                let key_val = entry.get_field(key_field).or_else(|| {
                    default_value(key_field)
                }).ok_or_else(|| {
                    serde::ser::Error::custom("Map entry key field missing and no default value")
                })?;
                let value_val = entry.get_field(value_field).or_else(|| {
                    default_value(value_field)
                });
                let map_key = match key_val {
                    Value::Bool(v) => MapKey::Bool(v),
                    Value::Int32(v) => MapKey::Int32(v),
                    Value::Int64(v) => MapKey::Int64(v),
                    Value::UInt32(v) => MapKey::UInt32(v),
                    Value::UInt64(v) => MapKey::UInt64(v),
                    Value::String(v) => MapKey::String(v.to_string()),
                    _ => {
                        return Err(serde::ser::Error::custom(
                            "Invalid map key type; must be scalar",
                        ))
                    }
                };
                if !seen_keys.insert(map_key) {
                    continue; // Skip duplicate keys, keep the last one
                }
                map_serializer.serialize_entry(&key_val, &value_val)?;
            }
            return map_serializer.end();
        }
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
        match *self {
            Value::Bool(v) => serializer.serialize_bool(v),
            Value::Int32(v) => serializer.serialize_i32(v),
            Value::Int64(v) => serializer.serialize_i64(v),
            Value::UInt32(v) => serializer.serialize_u32(v),
            Value::UInt64(v) => serializer.serialize_u64(v),
            Value::Float(v) => {
                if v.is_nan() {
                    serializer.serialize_str("NaN")
                } else if v.is_infinite() {
                    if v.is_sign_positive() {
                        serializer.serialize_str("Infinity")
                    } else {
                        serializer.serialize_str("-Infinity")
                    }
                } else {
                    serializer.serialize_f32(v)
                }
            }
            Value::Double(v) => {
                if v.is_nan() {
                    serializer.serialize_str("NaN")
                } else if v.is_infinite() {
                    if v.is_sign_positive() {
                        serializer.serialize_str("Infinity")
                    } else {
                        serializer.serialize_str("-Infinity")
                    }
                } else {
                    serializer.serialize_f64(v)
                }
            }
            Value::String(v) => serializer.serialize_str(v),
            Value::Bytes(v) => {
                if serializer.is_human_readable() {
                    use base64::Engine;
                    serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(v))
                } else {
                    serializer.serialize_bytes(v)
                }
            }
            Value::Message(ref msg) => msg.serialize(serializer),
            Value::RepeatedBool(list) => list.serialize(serializer),
            Value::RepeatedInt32(list) => list.serialize(serializer),
            Value::RepeatedInt64(list) => list.serialize(serializer),
            Value::RepeatedUInt32(list) => list.serialize(serializer),
            Value::RepeatedUInt64(list) => list.serialize(serializer),
            Value::RepeatedFloat(list) => list.serialize(serializer),
            Value::RepeatedDouble(list) => list.serialize(serializer),
            Value::RepeatedString(list) => list.serialize(serializer),
            Value::RepeatedBytes(list) => list.serialize(serializer),
            Value::RepeatedMessage(ref list) => list.serialize(serializer),
        }
    }
}

impl serde::Serialize for crate::containers::Bytes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            use base64::Engine;
            serializer
                .serialize_str(&base64::engine::general_purpose::STANDARD.encode(self.as_ref()))
        } else {
            serializer.serialize_bytes(self.as_ref())
        }
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
        let ProtobufVisitor { obj, table, arena } = self;
        serde_deserialize_struct(obj, table, arena, deserializer)?;
        Ok(())
    }
}

pub struct Optional<T>(T);

impl<'de, T> serde::de::DeserializeSeed<'de> for Optional<T>
where
    T: serde::de::DeserializeSeed<'de>,
{
    type Value = Option<T::Value>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_option(self)
    }
}

impl<'de, T> serde::de::Visitor<'de> for Optional<T>
where
    T: serde::de::DeserializeSeed<'de>,
{
    type Value = Option<T::Value>;

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("optional value")
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Some(self.0.deserialize(deserializer)?))
    }
}

pub fn serde_deserialize_struct<'arena, 'alloc, 'b, 'de, D>(
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
    let field_names_static = unsafe {
        std::mem::transmute::<&[&'static str], &'static [&'static str]>(field_names_slice)
    };
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
        loop {
            let msg_obj = Object::create(table.size as u32, arena);

            let seed = ProtobufVisitor {
                obj: msg_obj,
                table,
                arena,
            };

            match seq.next_element_seed(seed)? {
                Some(()) => {
                    rf.push(crate::base::Message(msg_obj as *mut Object), arena);
                }
                None => {
                    return Ok(());
                }
            }
        }
    }
}

struct ProtobufMapVisitor<'arena, 'alloc, 'b> {
    rf: &'b mut crate::containers::RepeatedField<crate::base::Message>,
    table: &'static Table,
    arena: &'arena mut crate::arena::Arena<'alloc>,
}

impl<'de, 'arena, 'alloc, 'b> serde::de::DeserializeSeed<'de>
    for ProtobufMapVisitor<'arena, 'alloc, 'b>
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de, 'arena, 'alloc, 'b> serde::de::Visitor<'de> for ProtobufMapVisitor<'arena, 'alloc, 'b> {
    type Value = ();

    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str(&format!("a map with {} entries", self.table.descriptor.name()))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let ProtobufMapVisitor { rf, table, arena } = self;

        let key_field = table.descriptor.field()[0];
        let value_field = table.descriptor.field()[1];
        let key_entry = table.entry(1).ok_or_else(|| {
            serde::de::Error::custom("Map entry missing key field in table")
        })?;
        let value_entry = table.entry(2).ok_or_else(|| {
            serde::de::Error::custom("Map entry missing value field in table")
        })?;

        while let Some(key_str) = map.next_key::<std::string::String>()? {
            let entry_obj = Object::create(table.size as u32, arena);

            match key_field.r#type().unwrap() {
                Type::TYPE_BOOL => {
                    let key_val: bool = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<bool>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 => {
                    let key_val: i32 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<i32>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let key_val: i64 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<i64>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let key_val: u32 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<u32>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let key_val: u64 = key_str.parse().map_err(serde::de::Error::custom)?;
                    entry_obj.set::<u64>(key_entry.offset(), key_entry.has_bit_idx(), key_val);
                }
                Type::TYPE_STRING => {
                    let s = crate::containers::String::from_str(&key_str, arena);
                    entry_obj.set::<crate::containers::String>(
                        key_entry.offset(),
                        key_entry.has_bit_idx(),
                        s,
                    );
                }
                _ => {
                    return Err(serde::de::Error::custom(format!(
                        "Unsupported map key type: {:?}",
                        key_field.r#type()
                    )));
                }
            }

            match value_field.r#type().unwrap() {
                Type::TYPE_BOOL => {
                    let v: bool = map.next_value()?;
                    entry_obj.set::<bool>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_INT32 | Type::TYPE_SINT32 | Type::TYPE_SFIXED32 | Type::TYPE_ENUM => {
                    let v: i32 = map.next_value()?;
                    entry_obj.set::<i32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_INT64 | Type::TYPE_SINT64 | Type::TYPE_SFIXED64 => {
                    let v: i64 = map.next_value()?;
                    entry_obj.set::<i64>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_UINT32 | Type::TYPE_FIXED32 => {
                    let v: u32 = map.next_value()?;
                    entry_obj.set::<u32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_UINT64 | Type::TYPE_FIXED64 => {
                    let v: u64 = map.next_value()?;
                    entry_obj.set::<u64>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_FLOAT => {
                    let v: f32 = map.next_value()?;
                    entry_obj.set::<f32>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_DOUBLE => {
                    let v: f64 = map.next_value()?;
                    entry_obj.set::<f64>(value_entry.offset(), value_entry.has_bit_idx(), v);
                }
                Type::TYPE_STRING => {
                    let v: std::string::String = map.next_value()?;
                    let s = crate::containers::String::from_str(&v, arena);
                    entry_obj.set::<crate::containers::String>(
                        value_entry.offset(),
                        value_entry.has_bit_idx(),
                        s,
                    );
                }
                Type::TYPE_BYTES => {
                    let v: BytesOrBase64 = map.next_value()?;
                    let b = crate::containers::Bytes::from_slice(&v.0, arena);
                    entry_obj.set::<crate::containers::Bytes>(
                        value_entry.offset(),
                        value_entry.has_bit_idx(),
                        b,
                    );
                }
                Type::TYPE_MESSAGE | Type::TYPE_GROUP => {
                    let value_aux = table.aux_entry_decode(value_entry);
                    let value_child_table = unsafe { &*value_aux.child_table };
                    let child_obj = Object::create(value_child_table.size as u32, arena);
                    let seed = ProtobufVisitor {
                        obj: child_obj,
                        table: value_child_table,
                        arena,
                    };
                    map.next_value_seed(seed)?;
                    *entry_obj.ref_mut::<crate::base::Message>(value_aux.offset) =
                        crate::base::Message(child_obj);
                }
            }

            rf.push(crate::base::Message(entry_obj as *mut Object), arena);
        }
        Ok(())
    }
}

// Flexible numeric deserializers - accept both numbers and quoted strings
struct FlexibleI32;
impl<'de> serde::de::DeserializeSeed<'de> for FlexibleI32 {
    type Value = i32;
    fn deserialize<D>(self, deserializer: D) -> Result<i32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleI32Visitor)
    }
}

struct FlexibleI32Visitor;
impl<'de> serde::de::Visitor<'de> for FlexibleI32Visitor {
    type Value = i32;
    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("i32 or string")
    }
    fn visit_i32<E>(self, v: i32) -> Result<i32, E> { Ok(v) }
    fn visit_i64<E>(self, v: i64) -> Result<i32, E> { Ok(v as i32) }
    fn visit_u64<E>(self, v: u64) -> Result<i32, E> { Ok(v as i32) }
    fn visit_f64<E>(self, v: f64) -> Result<i32, E> { Ok(v as i32) }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<i32, E> {
        v.parse().map_err(E::custom)
    }
}

struct FlexibleI64;
impl<'de> serde::de::DeserializeSeed<'de> for FlexibleI64 {
    type Value = i64;
    fn deserialize<D>(self, deserializer: D) -> Result<i64, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleI64Visitor)
    }
}

struct FlexibleI64Visitor;
impl<'de> serde::de::Visitor<'de> for FlexibleI64Visitor {
    type Value = i64;
    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("i64 or string")
    }
    fn visit_i64<E>(self, v: i64) -> Result<i64, E> { Ok(v) }
    fn visit_u64<E>(self, v: u64) -> Result<i64, E> { Ok(v as i64) }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<i64, E> {
        v.parse().map_err(E::custom)
    }
}

struct FlexibleU32;
impl<'de> serde::de::DeserializeSeed<'de> for FlexibleU32 {
    type Value = u32;
    fn deserialize<D>(self, deserializer: D) -> Result<u32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleU32Visitor)
    }
}

struct FlexibleU32Visitor;
impl<'de> serde::de::Visitor<'de> for FlexibleU32Visitor {
    type Value = u32;
    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("u32 or string")
    }
    fn visit_u32<E>(self, v: u32) -> Result<u32, E> { Ok(v) }
    fn visit_u64<E>(self, v: u64) -> Result<u32, E> { Ok(v as u32) }
    fn visit_i64<E>(self, v: i64) -> Result<u32, E> { Ok(v as u32) }
    fn visit_f64<E>(self, v: f64) -> Result<u32, E> { Ok(v as u32) }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u32, E> {
        v.parse().map_err(E::custom)
    }
}

struct FlexibleU64;
impl<'de> serde::de::DeserializeSeed<'de> for FlexibleU64 {
    type Value = u64;
    fn deserialize<D>(self, deserializer: D) -> Result<u64, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleU64Visitor)
    }
}

struct FlexibleU64Visitor;
impl<'de> serde::de::Visitor<'de> for FlexibleU64Visitor {
    type Value = u64;
    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("u64 or string")
    }
    fn visit_u64<E>(self, v: u64) -> Result<u64, E> { Ok(v) }
    fn visit_i64<E>(self, v: i64) -> Result<u64, E> { Ok(v as u64) }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u64, E> {
        v.parse().map_err(E::custom)
    }
}

struct FlexibleFloat;
impl<'de> serde::de::DeserializeSeed<'de> for FlexibleFloat {
    type Value = f32;
    fn deserialize<D>(self, deserializer: D) -> Result<f32, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleFloatVisitor)
    }
}

struct FlexibleFloatVisitor;
impl<'de> serde::de::Visitor<'de> for FlexibleFloatVisitor {
    type Value = f32;
    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("f32, string, NaN, or Infinity")
    }
    fn visit_f32<E>(self, v: f32) -> Result<f32, E> { Ok(v) }
    fn visit_f64<E>(self, v: f64) -> Result<f32, E> { Ok(v as f32) }
    fn visit_i64<E>(self, v: i64) -> Result<f32, E> { Ok(v as f32) }
    fn visit_u64<E>(self, v: u64) -> Result<f32, E> { Ok(v as f32) }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<f32, E> {
        match v {
            "NaN" => Ok(f32::NAN),
            "Infinity" => Ok(f32::INFINITY),
            "-Infinity" => Ok(f32::NEG_INFINITY),
            _ => v.parse().map_err(E::custom),
        }
    }
}

struct FlexibleDouble;
impl<'de> serde::de::DeserializeSeed<'de> for FlexibleDouble {
    type Value = f64;
    fn deserialize<D>(self, deserializer: D) -> Result<f64, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(FlexibleDoubleVisitor)
    }
}

struct FlexibleDoubleVisitor;
impl<'de> serde::de::Visitor<'de> for FlexibleDoubleVisitor {
    type Value = f64;
    fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        formatter.write_str("f64, string, NaN, or Infinity")
    }
    fn visit_f64<E>(self, v: f64) -> Result<f64, E> { Ok(v) }
    fn visit_f32<E>(self, v: f32) -> Result<f64, E> { Ok(v as f64) }
    fn visit_i64<E>(self, v: i64) -> Result<f64, E> { Ok(v as f64) }
    fn visit_u64<E>(self, v: u64) -> Result<f64, E> { Ok(v as f64) }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<f64, E> {
        match v {
            "NaN" => Ok(f64::NAN),
            "Infinity" => Ok(f64::INFINITY),
            "-Infinity" => Ok(f64::NEG_INFINITY),
            _ => v.parse().map_err(E::custom),
        }
    }
}

// Wrapper deserialization helpers
fn deserialize_wrapper<'de, 'arena, 'alloc, A, T>(
    obj: &mut Object,
    table: &'static Table,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
    T: serde::de::Deserialize<'de> + Copy,
{
    let entry = table.entry(1).ok_or_else(|| {
        serde::de::Error::custom("Wrapper missing 'value' field in table")
    })?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        if key == "value" {
            let val: T = map.next_value()?;
            obj.set::<T>(entry.offset(), entry.has_bit_idx(), val);
            return Ok(());
        } else {
            map.next_value::<serde::de::IgnoredAny>()?;
        }
    }
    Ok(())
}

fn deserialize_wrapper_string<'de, 'arena, 'alloc, A>(
    obj: &mut Object,
    table: &'static Table,
    arena: &mut crate::arena::Arena<'alloc>,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let entry = table.entry(1).ok_or_else(|| {
        serde::de::Error::custom("StringValue missing 'value' field in table")
    })?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        if key == "value" {
            let val: std::string::String = map.next_value()?;
            let s = crate::containers::String::from_str(&val, arena);
            obj.set::<crate::containers::String>(entry.offset(), entry.has_bit_idx(), s);
            return Ok(());
        } else {
            map.next_value::<serde::de::IgnoredAny>()?;
        }
    }
    Ok(())
}

fn deserialize_wrapper_bytes<'de, 'arena, 'alloc, A>(
    obj: &mut Object,
    table: &'static Table,
    arena: &mut crate::arena::Arena<'alloc>,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let entry = table.entry(1).ok_or_else(|| {
        serde::de::Error::custom("BytesValue missing 'value' field in table")
    })?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        if key == "value" {
            let val: BytesOrBase64 = map.next_value()?;
            let b = crate::containers::Bytes::from_slice(&val.0, arena);
            obj.set::<crate::containers::Bytes>(entry.offset(), entry.has_bit_idx(), b);
            return Ok(());
        } else {
            map.next_value::<serde::de::IgnoredAny>()?;
        }
    }
    Ok(())
}

fn deserialize_timestamp<'de, 'arena, 'alloc, A>(
    obj: &mut Object,
    table: &'static Table,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let seconds_entry = table.entry(1).ok_or_else(|| {
        serde::de::Error::custom("Timestamp missing 'seconds' field in table")
    })?;
    let nanos_entry = table.entry(2).ok_or_else(|| {
        serde::de::Error::custom("Timestamp missing 'nanos' field in table")
    })?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        match key.as_str() {
            "seconds" => {
                let s: i64 = map.next_value()?;
                obj.set::<i64>(seconds_entry.offset(), seconds_entry.has_bit_idx(), s);
            }
            "nanos" => {
                let n: i32 = map.next_value()?;
                obj.set::<i32>(nanos_entry.offset(), nanos_entry.has_bit_idx(), n);
            }
            _ => {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
    }

    // Validate after parsing
    let seconds = obj.get::<i64>(seconds_entry.offset() as usize);
    let nanos = obj.get::<i32>(nanos_entry.offset() as usize);
    validate_timestamp(seconds, nanos).map_err(serde::de::Error::custom)?;

    Ok(())
}

fn deserialize_duration<'de, 'arena, 'alloc, A>(
    obj: &mut Object,
    table: &'static Table,
    mut map: A,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
{
    let seconds_entry = table.entry(1).ok_or_else(|| {
        serde::de::Error::custom("Duration missing 'seconds' field in table")
    })?;
    let nanos_entry = table.entry(2).ok_or_else(|| {
        serde::de::Error::custom("Duration missing 'nanos' field in table")
    })?;

    while let Some(key) = map.next_key::<std::string::String>()? {
        match key.as_str() {
            "seconds" => {
                let s: i64 = map.next_value()?;
                obj.set::<i64>(seconds_entry.offset(), seconds_entry.has_bit_idx(), s);
            }
            "nanos" => {
                let n: i32 = map.next_value()?;
                obj.set::<i32>(nanos_entry.offset(), nanos_entry.has_bit_idx(), n);
            }
            _ => {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
    }

    // Validate after parsing
    let seconds = obj.get::<i64>(seconds_entry.offset() as usize);
    let nanos = obj.get::<i32>(nanos_entry.offset() as usize);
    validate_duration(seconds, nanos).map_err(serde::de::Error::custom)?;

    Ok(())
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

        // Check if this is a well-known type
        match detect_well_known_type(table.descriptor) {
            WellKnownType::BoolValue => return deserialize_wrapper::<A, bool>(obj, table, map),
            WellKnownType::Int32Value => return deserialize_wrapper::<A, i32>(obj, table, map),
            WellKnownType::Int64Value => return deserialize_wrapper::<A, i64>(obj, table, map),
            WellKnownType::UInt32Value => return deserialize_wrapper::<A, u32>(obj, table, map),
            WellKnownType::UInt64Value => return deserialize_wrapper::<A, u64>(obj, table, map),
            WellKnownType::FloatValue => return deserialize_wrapper::<A, f32>(obj, table, map),
            WellKnownType::DoubleValue => return deserialize_wrapper::<A, f64>(obj, table, map),
            WellKnownType::StringValue => return deserialize_wrapper_string(obj, table, arena, map),
            WellKnownType::BytesValue => return deserialize_wrapper_bytes(obj, table, arena, map),
            WellKnownType::Timestamp => return deserialize_timestamp(obj, table, map),
            WellKnownType::Duration => return deserialize_duration(obj, table, map),
            WellKnownType::None => {
                // Continue with regular deserialization
            }
        }

        let mut field_map = std::collections::HashMap::new();
        for (field_index, field) in table.descriptor.field().iter().enumerate() {
            let field_name = field.json_name();
            field_map.insert(field_name, field_index);
        }
        while let Some(idx) = map.next_key_seed(StructKeyVisitor(&field_map))? {
            let field = table.descriptor.field()[idx];
            let entry = table.entry(field.number() as u32).unwrap(); // Safe: field exists in table
            match field.label().unwrap() {
                Label::LABEL_REPEATED => match field.r#type().unwrap() {
                    Type::TYPE_BOOL => {
                        let Some(slice) = map.next_value::<Option<Vec<bool>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<bool>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FIXED64 | Type::TYPE_UINT64 => {
                        let Some(slice) = map.next_value::<Option<Vec<u64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<u64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let Some(slice) = map.next_value::<Option<Vec<u32>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<u32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let Some(slice) = map.next_value::<Option<Vec<i64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<i64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_SFIXED32
                    | Type::TYPE_INT32
                    | Type::TYPE_SINT32
                    | Type::TYPE_ENUM => {
                        let Some(slice) = map.next_value::<Option<Vec<i32>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<i32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_FLOAT => {
                        let Some(slice) = map.next_value::<Option<Vec<f32>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<f32>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_DOUBLE => {
                        let Some(slice) = map.next_value::<Option<Vec<f64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            obj.add::<f64>(entry.offset(), v, arena);
                        }
                    }
                    Type::TYPE_STRING => {
                        let Some(slice) = map.next_value::<Option<Vec<String>>>()? else {
                            continue;
                        };
                        for v in slice {
                            let s = crate::containers::String::from_str(&v, arena);
                            obj.add::<crate::containers::String>(entry.offset(), s, arena);
                        }
                    }
                    Type::TYPE_BYTES => {
                        let Some(slice) = map.next_value::<Option<Vec<BytesOrBase64>>>()? else {
                            continue;
                        };
                        for v in slice {
                            let b = crate::containers::Bytes::from_slice(&v.0, arena);
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

                        if child_table.descriptor.options().map(|o| o.map_entry()).unwrap_or(false) {
                            let seed = Optional(ProtobufMapVisitor {
                                rf,
                                table: child_table,
                                arena,
                            });
                            map.next_value_seed(seed)?;
                        } else {
                            let seed = Optional(ProtobufArrayfVisitor {
                                rf,
                                table: child_table,
                                arena,
                            });
                            map.next_value_seed(seed)?;
                        }
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
                        let Some(v) = map.next_value_seed(Optional(FlexibleU64))? else {
                            continue;
                        };
                        obj.set::<u64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FIXED32 | Type::TYPE_UINT32 => {
                        let Some(v) = map.next_value_seed(Optional(FlexibleU32))? else {
                            continue;
                        };
                        obj.set::<u32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED64 | Type::TYPE_INT64 | Type::TYPE_SINT64 => {
                        let Some(v) = map.next_value_seed(Optional(FlexibleI64))? else {
                            continue;
                        };
                        obj.set::<i64>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_SFIXED32
                    | Type::TYPE_INT32
                    | Type::TYPE_SINT32
                    | Type::TYPE_ENUM => {
                        let Some(v) = map.next_value_seed(Optional(FlexibleI32))? else {
                            continue;
                        };
                        obj.set::<i32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_FLOAT => {
                        let Some(v) = map.next_value_seed(Optional(FlexibleFloat))? else {
                            continue;
                        };
                        obj.set::<f32>(entry.offset(), entry.has_bit_idx(), v);
                    }
                    Type::TYPE_DOUBLE => {
                        let Some(v) = map.next_value_seed(Optional(FlexibleDouble))? else {
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
                        let Some(v) = map.next_value::<Option<BytesOrBase64>>()? else {
                            continue;
                        };
                        let b = crate::containers::Bytes::from_slice(&v.0, arena);
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
                        let seed = Optional(ProtobufVisitor {
                            obj: child_obj,
                            table: child_table,
                            arena,
                        });
                        if map.next_value_seed(seed)?.is_none() {
                            continue;
                        };
                        *obj.ref_mut::<crate::base::Message>(offset) =
                            crate::base::Message(child_obj);
                    }
                },
            }
        }
        Ok(())
    }
}

struct BytesOrBase64(Vec<u8>);

impl<'de> serde::de::Deserialize<'de> for BytesOrBase64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BytesOrStringVisitor;

        impl<'de> serde::de::Visitor<'de> for BytesOrStringVisitor {
            type Value = BytesOrBase64;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("bytes or string")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(BytesOrBase64(v.to_vec()))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(v)
                    .map(|bytes| BytesOrBase64(bytes))
                    .map_err(|err| {
                        serde::de::Error::custom(format!("Invalid base64 string: {}", err))
                    })
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let bytes: Vec<u8> = serde::de::Deserialize::deserialize(
                    serde::de::value::SeqAccessDeserializer::new(seq),
                )?;
                Ok(BytesOrBase64(bytes))
            }
        }

        deserializer.deserialize_any(BytesOrStringVisitor)
    }
}
