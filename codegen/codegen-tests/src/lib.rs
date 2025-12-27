#![feature(allocator_api)]

#[cfg(test)]
use protocrap::tests::assert_roundtrip;
#[cfg(test)]
use protocrap::{Protobuf, ProtobufRef};
use protocrap::{self, containers::Bytes};
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

#[cfg(test)]
fn assert_json_roundtrip<T: Protobuf>(msg: &T) {
    let serialized = serde_json::to_string(&protocrap::reflection::DynamicMessageRef::new(msg))
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
    assert_json_roundtrip(
        protocrap::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor(),
    );
}

// Chunked streaming tests
#[cfg(test)]
mod chunked_tests {
    use super::*;
    use protocrap::decoding::ResumeableDecode;
    use rand::{Rng, SeedableRng, rngs::StdRng};

    #[derive(Clone, Copy, Debug)]
    enum ChunkStrategy {
        Uniform(usize),
        SmallOnly,
        LargeOnly,
        Alternating,
        Random,
    }

    struct ChunkIter<'a> {
        data: &'a [u8],
        pos: usize,
        strategy: ChunkStrategy,
        rng: StdRng,
        toggle: bool,
    }

    impl<'a> ChunkIter<'a> {
        fn new(data: &'a [u8], strategy: ChunkStrategy, seed: u64) -> Self {
            Self {
                data,
                pos: 0,
                strategy,
                rng: StdRng::seed_from_u64(seed),
                toggle: false,
            }
        }

        fn next_chunk_size(&mut self) -> usize {
            match self.strategy {
                ChunkStrategy::Uniform(size) => size,
                ChunkStrategy::SmallOnly => self.rng.gen_range(1..16),
                ChunkStrategy::LargeOnly => self.rng.gen_range(16..129),
                ChunkStrategy::Alternating => {
                    self.toggle = !self.toggle;
                    if self.toggle {
                        self.rng.gen_range(1..16)
                    } else {
                        self.rng.gen_range(16..129)
                    }
                }
                ChunkStrategy::Random => self.rng.gen_range(1..129),
            }
        }
    }

    impl<'a> Iterator for ChunkIter<'a> {
        type Item = &'a [u8];

        fn next(&mut self) -> Option<Self::Item> {
            if self.pos >= self.data.len() {
                return None;
            }
            let size = self.next_chunk_size();
            let end = (self.pos + size).min(self.data.len());
            let chunk = &self.data[self.pos..end];
            self.pos = end;
            Some(chunk)
        }
    }

    fn random_string(rng: &mut impl Rng, max_len: usize) -> String {
        let len = rng.gen_range(0..=max_len);
        (0..len)
            .map(|_| rng.gen_range(b'a'..=b'z') as char)
            .collect()
    }

    fn make_random(
        arena: &mut protocrap::arena::Arena,
        rng: &mut impl Rng,
        depth: u8,
    ) -> TestProto {
        let mut msg = TestProto::default();

        if rng.r#gen() {
            msg.set_x(rng.r#gen());
        }
        if rng.r#gen() {
            msg.set_y(rng.r#gen());
        }
        if rng.r#gen() {
            let s = random_string(rng, 100);
            msg.set_z(&s, arena);
        }
        if depth > 0 && rng.gen_bool(0.3) {
            let child = msg.child1_mut(arena);
            *child = make_random(arena, rng, depth - 1);
        }
        if depth > 0 && rng.gen_bool(0.3) {
            let count = rng.gen_range(0..5);
            for _ in 0..count {
                let nested = msg.add_nested_message(arena);
                if rng.r#gen() {
                    nested.set_x(rng.r#gen());
                }
            }
        }
        if rng.gen_bool(0.3) {
            let count = rng.gen_range(0..5);
            for _ in 0..count {
                let bytes_data: Vec<u8> = (0..rng.gen_range(0..50)).map(|_| rng.r#gen()).collect();
                msg.rep_bytes_mut().push(
                    protocrap::containers::Bytes::from_slice(&bytes_data, arena),
                    arena,
                );
            }
        }

        msg
    }

    fn assert_chunked_decode(msg: &TestProto, strategy: ChunkStrategy, chunk_seed: u64) {
        let encoded = msg.encode_vec::<32>().expect("encode should succeed");
        let chunks = ChunkIter::new(&encoded, strategy, chunk_seed);

        let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
        let mut decoded = TestProto::default();
        let mut decoder = ResumeableDecode::<32>::new(&mut decoded, isize::MAX);

        for chunk in chunks {
            if !decoder.resume(chunk, &mut arena) {
                panic!(
                    "decode failed at chunk, strategy={:?}, chunk_seed={}",
                    strategy, chunk_seed
                );
            }
        }
        if !decoder.finish(&mut arena) {
            panic!(
                "decode finish failed, strategy={:?}, chunk_seed={}",
                strategy, chunk_seed
            );
        }

        let reencoded = decoded.encode_vec::<32>().expect("reencode should succeed");
        assert_eq!(
            encoded, reencoded,
            "roundtrip mismatch, strategy={:?}, chunk_seed={}",
            strategy, chunk_seed
        );
    }

    #[test]
    fn test_chunked_decode_fixed_messages() {
        let strategies = [
            ChunkStrategy::Uniform(1),
            ChunkStrategy::Uniform(7),
            ChunkStrategy::Uniform(16),
            ChunkStrategy::SmallOnly,
            ChunkStrategy::LargeOnly,
            ChunkStrategy::Alternating,
            ChunkStrategy::Random,
        ];

        let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);

        for strategy in strategies {
            for seed in 0..10 {
                assert_chunked_decode(&make_small(), strategy, seed);
                assert_chunked_decode(&make_medium(&mut arena), strategy, seed);
                assert_chunked_decode(&make_large(&mut arena), strategy, seed);
            }
        }
    }

    #[test]
    fn test_chunked_decode_random_messages() {
        let strategies = [
            ChunkStrategy::SmallOnly,
            ChunkStrategy::LargeOnly,
            ChunkStrategy::Alternating,
            ChunkStrategy::Random,
        ];

        for strategy in strategies {
            for msg_seed in 0..50 {
                let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
                let mut rng = StdRng::seed_from_u64(msg_seed);
                let msg = make_random(&mut arena, &mut rng, 3);

                for chunk_seed in 0..5 {
                    assert_chunked_decode(&msg, strategy, chunk_seed);
                }
            }
        }
    }
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
    assert_eq!(msg.defaulted_bytes(), b"My \0 byte array");

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
