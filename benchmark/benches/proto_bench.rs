#![feature(allocator_api)]

use criterion::{
    BenchmarkGroup, Criterion, Throughput, black_box, criterion_group, criterion_main,
    measurement::Measurement,
};
use prost::Message;

// Your crate
use codegen_tests::{
    Test::ProtoType as Test, make_large, make_medium, make_small
};
use protocrap::{ProtobufExt, arena};

mod prost_gen {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

fn bench_decoding(
    group: &mut BenchmarkGroup<'_, impl Measurement>,
    bench_function_name: &str,
    data: &[u8],
) {
    group.throughput(Throughput::Bytes(data.len() as u64));

    group.bench_function(&format!("{}/protocrap", bench_function_name), |b| {
        let mut arena = crate::arena::Arena::new(&std::alloc::Global);
        let mut msg = Test::default();
        b.iter(|| {
            msg.nested_message_mut().clear();
            let _ = msg.decode_flat::<32>(&mut arena, black_box(data));
            black_box(&msg as *const _);
        })
    });

    group.bench_function(&format!("{}/prost", bench_function_name), |b| {
        b.iter(|| {
            let msg = prost_gen::Test::decode(black_box(data)).unwrap();
            black_box(msg)
        })
    });
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");

    // Small message
    let small_data = make_small().encode_vec::<32>().expect("should encode");
    bench_decoding(&mut group, "small", &small_data);

    // Medium message
    let mut medium_arena = arena::Arena::new(&std::alloc::Global);
    let medium_data = make_medium(&mut medium_arena).encode_vec::<32>().expect("should encode");
    bench_decoding(&mut group, "medium", &medium_data);

    // Large message
    let mut large_arena = arena::Arena::new(&std::alloc::Global);
    let large_data = make_large(&mut large_arena).encode_vec::<32>().expect("should encode");
    bench_decoding(&mut group, "large", &large_data);

    group.finish();
}

fn bench_encoding(
    c: &mut BenchmarkGroup<'_, impl Measurement>,
    bench_function_name: &str,
    protocrap_msg: &Test,
) {
    let data = protocrap_msg.encode_vec::<32>().expect("should encode");
    let prost_msg = prost_gen::Test::decode(data.as_slice()).unwrap();

    c.bench_function(&format!("{}/protocrap", bench_function_name), |b| {
        let mut buf = vec![0u8; 4096];
        b.iter(|| {
            let result = protocrap_msg
                .encode_flat::<32>(black_box(&mut buf))
                .unwrap();
            black_box(result.len())
        })
    });

    c.bench_function(&format!("{}/prost", bench_function_name), |b| {
        let mut buf = Vec::with_capacity(4096);
        b.iter(|| {
            buf.clear();
            prost_msg.encode(black_box(&mut buf)).unwrap();
            black_box(buf.len())
        })
    });
}

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode");

    // Small
    let small_prost = make_small();
    bench_encoding(&mut group, "small", &small_prost);

    // Medium
    let mut medium_arena = arena::Arena::new(&std::alloc::Global);
    let medium_prost = make_medium(&mut medium_arena);
    bench_encoding(&mut group, "medium", &medium_prost);

    // Large
    let mut large_arena = arena::Arena::new(&std::alloc::Global);
    let large_prost = make_large(&mut large_arena);
    bench_encoding(&mut group, "large", &large_prost);

    group.finish();
}

criterion_group!(benches, bench_decode, bench_encode);
criterion_main!(benches);
