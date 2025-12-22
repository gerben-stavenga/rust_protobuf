#![feature(allocator_api)]

#[cfg(test)]
use protocrap::tests::assert_roundtrip;
use protocrap::{self, Protobuf, ProtobufExt, containers::Bytes};
include!(concat!(env!("OUT_DIR"), "/test.pc.rs"));

use Test::ProtoType as TestProto;

pub fn make_small() -> TestProto {
    let mut msg = TestProto::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg
}

pub fn make_medium(arena: &mut protocrap::arena::Arena) -> TestProto {
    let mut msg = TestProto::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg.set_z(
        "Hello World! This is a test string with some content.",
        arena,
    );
    let child1 = msg.child1_mut(arena);
    child1.set_x(123);
    child1.set_y(456);
    msg
}

pub fn make_large(arena: &mut protocrap::arena::Arena) -> TestProto {
    let mut msg = TestProto::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg.set_z("Hello World!", arena);
    for i in 0..100 {
        let nested_msg = msg.add_nested_message(arena);
        nested_msg.set_x(i);
    }
    for i in 0..5 {
        msg.rep_bytes_mut().push(
            Bytes::from_slice(format!("byte array number {}", i).as_bytes(), arena),
            arena,
        );
    }
    msg
}

fn assert_json_roundtrip<T: Protobuf>(msg: &T) {
    let serialized = serde_json::to_string(&protocrap::reflection::DynamicMessage::new(msg))
        .expect("should serialize");

    println!("Serialized JSON: {}", serialized);

    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let roundtrip_msg = {
        let mut deserializer = serde_json::Deserializer::from_str(&serialized);
        let seed = protocrap::serde::SerdeDeserialize::<T>::new(&mut arena);
        use serde::de::DeserializeSeed;
        seed.deserialize(&mut deserializer)
            .expect("should deserialize")
    };
    let data = msg.encode_vec::<100>().expect("msg should encode");
    let roundtrip_data = roundtrip_msg
        .encode_vec::<100>()
        .expect("msg should encode");

    assert_eq!(roundtrip_data, data);
}

#[test]
fn test_small_roundtrips() {
    assert_roundtrip(&make_small());
}

#[test]
fn test_medium_roundtrips() {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    assert_roundtrip(&make_medium(&mut arena));
}

#[test]
fn test_large_roundtrips() {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    assert_roundtrip(&make_large(&mut arena));
}

#[test]
fn test_file_descriptor_roundtrip() {
    assert_roundtrip(&protocrap::google::protobuf::FILE_DESCRIPTOR_PROTO);
}

#[test]
fn test_small_serde_serialization() {
    assert_json_roundtrip(&make_small());
}

#[test]
fn test_medium_serde_serialization() {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    assert_json_roundtrip(&make_medium(&mut arena));
}

#[test]
fn test_large_serde_serialization() {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    assert_json_roundtrip(&make_large(&mut arena));
}

#[test]
fn test_file_descriptor_serde_serialization() {
    assert_json_roundtrip(&protocrap::google::protobuf::FILE_DESCRIPTOR_PROTO);
}

#[test]
fn test_defaults() {
    // Create a message without setting any fields
    let msg = DefaultsTest::ProtoType::default();

    // Verify default values are returned when fields are not set
    assert_eq!(msg.port(), 8080);
    assert_eq!(msg.enabled(), true);
    assert_eq!(msg.ratio(), 3.14);
    assert_eq!(msg.precise(), 2.71828);
    assert_eq!(msg.count(), 42);
    assert_eq!(msg.big_number(), -9223372036854775808);
    assert_eq!(msg.special_inf(), f32::INFINITY);
    assert_eq!(msg.special_neg_inf(), f32::NEG_INFINITY);
    assert!(msg.special_nan().is_nan());
    assert_eq!(msg.greeting(), "Hello, World!");
    assert_eq!(msg.multiline(), "Line1\nLine2\tTabbed");
    assert_eq!(msg.escaped(), "Quote: \" Backslash: \\");

    // Verify has_* methods return false for unset fields
    assert!(!msg.has_port());
    assert!(!msg.has_enabled());
    assert!(!msg.has_ratio());
    assert!(!msg.has_greeting());
    assert!(!msg.has_multiline());

    // Verify get_* methods return None for unset fields
    assert_eq!(msg.get_port(), None);
    assert_eq!(msg.get_enabled(), None);
    assert_eq!(msg.get_ratio(), None);
    assert_eq!(msg.get_greeting(), None);
    assert_eq!(msg.get_multiline(), None);

    // Test set/clear behavior
    let mut msg2 = DefaultsTest::ProtoType::default();

    // Set a field to a value different from default
    msg2.set_port(9000);
    assert_eq!(msg2.port(), 9000);
    assert!(msg2.has_port());
    assert_eq!(msg2.get_port(), Some(9000));

    // Clear the field - should return to default
    msg2.clear_port();
    assert_eq!(msg2.port(), 8080);
    assert!(!msg2.has_port());
    assert_eq!(msg2.get_port(), None);
}
