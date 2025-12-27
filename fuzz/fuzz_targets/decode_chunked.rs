#![no_main]
#![feature(allocator_api)]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use protocrap::decoding::ResumeableDecode;

#[derive(Arbitrary, Debug)]
struct ChunkedInput {
    data: Vec<u8>,
    chunk_sizes: Vec<u8>,
}

fuzz_target!(|input: ChunkedInput| {
    let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
    let mut msg = protocrap::google::protobuf::FileDescriptorProto::ProtoType::default();
    let mut decoder = ResumeableDecode::<32>::new(&mut msg, isize::MAX);

    let mut pos = 0;
    let mut chunk_idx = 0;

    while pos < input.data.len() {
        let size = input
            .chunk_sizes
            .get(chunk_idx)
            .copied()
            .unwrap_or(16)
            .max(1) as usize;
        let end = (pos + size).min(input.data.len());
        let chunk = &input.data[pos..end];

        if !decoder.resume(chunk, &mut arena) {
            return; // Decode error is fine for fuzz testing
        }

        pos = end;
        chunk_idx += 1;
    }

    let _ = decoder.finish(&mut arena);
});
