#![feature(allocator_api)]

use criterion::{
    BenchmarkGroup, Criterion, Throughput, black_box, criterion_group, criterion_main,
    measurement::Measurement,
};
use prost::Message;

// Your crate
use protocrap::{
    ProtobufExt, arena, tests::encode_prost, tests::make_large_prost, tests::make_medium_prost,
    tests::make_protocrap, tests::make_small_prost, tests::prost_gen, tests::test::Test,
};

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
            msg.parse_flat::<32>(&mut arena, black_box(data));
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
    let small_data = encode_prost(&make_small_prost());
    bench_decoding(&mut group, "small", &small_data);

    // Medium message
    let medium_data = encode_prost(&make_medium_prost());
    bench_decoding(&mut group, "medium", &medium_data);

    // Large message
    let large_data = encode_prost(&make_large_prost());
    bench_decoding(&mut group, "large", &large_data);

    group.finish();
}

fn bench_encoding(
    c: &mut BenchmarkGroup<'_, impl Measurement>,
    bench_function_name: &str,
    prost_msg: &prost_gen::Test,
) {
    let mut arena = crate::arena::Arena::new(&std::alloc::Global);
    let mut protocrap_msg = make_protocrap(prost_msg, &mut arena);

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
    let small_prost = make_small_prost();
    bench_encoding(&mut group, "small", &small_prost);

    // Medium
    let medium_prost = make_medium_prost();
    bench_encoding(&mut group, "medium", &medium_prost);

    // Large
    let large_prost = make_large_prost();
    bench_encoding(&mut group, "large", &large_prost);

    group.finish();
}

criterion_group!(benches, bench_decode, bench_encode);
criterion_main!(benches);
