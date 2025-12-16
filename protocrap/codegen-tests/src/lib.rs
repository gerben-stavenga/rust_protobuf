#![feature(allocator_api)]

use protocrap::{self, ProtobufExt};

include!(concat!(env!("OUT_DIR"), "/test.pc.rs"));

use prost::Message;
pub mod prost_gen {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

pub fn make_small_prost() -> prost_gen::Test {
    prost_gen::Test {
        x: Some(42),
        y: Some(0xDEADBEEF),
        z: None,
        child1: None,
        child2: None,
        nested_message: vec![],
    }
}

pub fn make_medium_prost() -> prost_gen::Test {
    prost_gen::Test {
        x: Some(42),
        y: Some(0xDEADBEEF),
        z: Some(b"Hello World! This is a test string with some content.".to_vec()),
        child1: Some(Box::new(prost_gen::Test {
            x: Some(123),
            y: Some(456),
            z: None,
            child1: None,
            child2: None,
            nested_message: vec![],
        })),
        child2: None,
        nested_message: vec![],
    }
}

pub fn make_large_prost() -> prost_gen::Test {
    prost_gen::Test {
        x: Some(42),
        y: Some(0xDEADBEEF),
        z: Some(b"Hello World!".to_vec()),
        child1: None,
        child2: None,
        nested_message: (0..100)
            .map(|i| prost_gen::test::NestedMessage {
                x: Some(i),
                recursive: None,
            })
            .collect(),
    }
}

pub fn encode_prost(msg: &prost_gen::Test) -> Vec<u8> {
    let mut buf = Vec::with_capacity(msg.encoded_len());
    msg.encode(&mut buf).unwrap();
    buf
}

pub fn make_protocrap(
    msg: &prost_gen::Test,
    arena: &mut protocrap::arena::Arena,
) -> Test::ProtoType {
    let mut protocrap_msg = Test::ProtoType::default();
    let data = encode_prost(msg);
    assert!(protocrap_msg.decode_flat::<32>(arena, &data));
    protocrap_msg
}

#[cfg(test)]
fn assert_roundtrip(msg: prost_gen::Test) {
    let data = encode_prost(&msg);

    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let mut protocrap_msg = Test::ProtoType::default();
    assert!(protocrap_msg.decode_flat::<32>(&mut arena, &data));

    let mut buffer = vec![0u8; data.len()];
    let written = protocrap_msg
        .encode_flat::<32>(&mut buffer)
        .expect("msg should encode");
    assert_eq!(written.len(), data.len());

    let decoded_prost = prost_gen::Test::decode(&written[..]).expect("should decode");
    let encoded_data = encode_prost(&decoded_prost);
    assert_eq!(encoded_data, data);
}

#[test]
fn test_small_roundtrips() {
    assert_roundtrip(make_small_prost());
}

#[test]
fn test_medium_roundtrips() {
    assert_roundtrip(make_medium_prost());
}

#[test]
fn test_large_roundtrips() {
    assert_roundtrip(make_large_prost());
}

#[test]
fn test_serde_serialization() {
    let msg = make_medium_prost();
    let data = encode_prost(&msg);

    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let mut protocrap_msg = Test::ProtoType::default();
    assert!(protocrap_msg.decode_flat::<32>(&mut arena, &data));

    let mut buffer = vec![0u8; data.len()];
    assert_eq!(protocrap_msg.encode_flat::<100>(&mut buffer).expect("msg should encode").len(), data.len());
    let serialized = serde_json::to_string(&protocrap::serde::SerdeProtobuf::new(&protocrap_msg))
        .expect("should serialize");

    println!("Serialized JSON: {}", serialized);

    let deserialized = {
        let mut deserializer = serde_json::Deserializer::from_str(&serialized);
        let seed = protocrap::serde::SerdeDeserialize::<Test::ProtoType>::new(&mut arena);
        use serde::de::DeserializeSeed;
        seed.deserialize(&mut deserializer).expect("should deserialize")
    };

    let mut buffer2 = vec![0u8; data.len()];
    assert_eq!(deserialized.encode_flat::<100>(&mut buffer2).expect("msg should encode").len(), data.len());
    assert_eq!(buffer2, buffer);
}
