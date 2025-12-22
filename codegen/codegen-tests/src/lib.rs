#![feature(allocator_api)]

use protocrap::{self, ProtobufExt};
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
        b"Hello World! This is a test string with some content.",
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
    msg.set_z(b"Hello World!", arena);
    /*for i in 0..100 {
        let mut nested_msg = msg.nested_message_mut();
        let elem = arena.alloc::<Test::NestedMessage::ProtoType>();
        unsafe { elem.write(Test::NestedMessage::ProtoType::default()) };
        nested_msg.push(protocrap::base::Message(elem as *mut _), arena);
        let child = msg.nested_message_mut(arena);
        child.set_x(i);
    }*/
    msg
}

#[cfg(test)]
fn assert_roundtrip(msg: &TestProto) {
    let data = msg.encode_vec::<32>().expect("msg should encode");

    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let mut roundtrip_msg = Test::ProtoType::default();
    assert!(roundtrip_msg.decode_flat::<32>(&mut arena, &data));

    println!("Roundtrip message: {:#?}", roundtrip_msg);

    let roundtrip_data = roundtrip_msg.encode_vec::<32>().expect("msg should encode");

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

#[cfg(test)]
fn assert_json_roundtrip(msg: TestProto) {
    let serialized = serde_json::to_string(&protocrap::serde::SerdeProtobuf::new(&msg))
        .expect("should serialize");

    println!("Serialized JSON: {}", serialized);

    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let roundtrip_msg = {
        let mut deserializer = serde_json::Deserializer::from_str(&serialized);
        let seed = protocrap::serde::SerdeDeserialize::<Test::ProtoType>::new(&mut arena);
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
fn test_small_serde_serialization() {
    let msg = make_small();
    assert_json_roundtrip(msg);
}

#[test]
fn test_medium_serde_serialization() {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let msg = make_medium(&mut arena);
    assert_json_roundtrip(msg);
}

#[test]
fn test_large_serde_serialization() {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let msg = make_large(&mut arena);
    assert_json_roundtrip(msg);
}
