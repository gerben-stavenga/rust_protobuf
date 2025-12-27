#![no_main]
#![feature(allocator_api)]

use libfuzzer_sys::fuzz_target;
use protocrap::ProtobufMut;

fuzz_target!(|data: &[u8]| {
    // Try to decode arbitrary bytes as a FileDescriptorProto
    // This tests that malformed input doesn't cause crashes
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let mut msg = protocrap::google::protobuf::FileDescriptorProto::ProtoType::default();
    let _ = msg.decode_flat::<32>(&mut arena, data);
});
