use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};

const PEANUTS_SPECIALS_JSON: &[u8] = include_bytes!("../benchmarks/data/peanuts_specials.json");

#[derive(Clone, Serialize, Deserialize)]
struct SpecialsContext {
    dataset: String,
    focus: String,
    source: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Special {
    id: u64,
    slug: String,
    title: String,
    #[serde(rename = "airDate")]
    air_date: String,
    network: String,
    #[serde(rename = "otherNetworks")]
    other_networks: Vec<String>,
    notes: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SpecialsDataset {
    context: SpecialsContext,
    sources: Vec<String>,
    specials: Vec<Special>,
}

fn load_specials() -> SpecialsDataset {
    serde_json::from_slice(PEANUTS_SPECIALS_JSON)
        .expect("failed to parse benchmarks/data/peanuts_specials.json")
}

fn bench_peanuts_specials(c: &mut Criterion) {
    let dataset = load_specials();
    let toon = serde_toon::to_string(&dataset).expect("encode failed");
    let json = serde_json::to_string(&dataset).expect("json encode failed");

    let mut group = c.benchmark_group("peanuts_specials");
    group.bench_function("encode_toon", |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(&dataset)).expect("encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_toon", |b| {
        b.iter(|| {
            let decoded: SpecialsDataset =
                serde_toon::from_str(black_box(&toon)).expect("decode failed");
            black_box(decoded);
        });
    });
    group.bench_function("encode_json", |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(black_box(&dataset)).expect("json encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_json", |b| {
        b.iter(|| {
            let decoded: SpecialsDataset =
                serde_json::from_str(black_box(&json)).expect("json decode failed");
            black_box(decoded);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_peanuts_specials);
criterion_main!(benches);
