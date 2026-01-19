use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};

const PEANUTS_JSONLD_JSON: &[u8] = include_bytes!("../benchmarks/data/peanuts_jsonld.json");

#[derive(Clone, Serialize, Deserialize)]
struct JsonLdNode {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@type")]
    type_name: String,
    name: String,
    url: String,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
    category: String,
    #[serde(rename = "isPartOf")]
    is_part_of: Option<String>,
    author: Option<String>,
    publisher: Option<String>,
    genre: Vec<String>,
    description: Option<String>,
    #[serde(rename = "sameAs")]
    same_as: Vec<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct JsonLdDataset {
    #[serde(rename = "@context")]
    context: String,
    #[serde(rename = "@graph")]
    graph: Vec<JsonLdNode>,
    sources: Vec<String>,
}

fn load_jsonld() -> JsonLdDataset {
    serde_json::from_slice(PEANUTS_JSONLD_JSON)
        .expect("failed to parse benchmarks/data/peanuts_jsonld.json")
}

fn bench_peanuts_jsonld(c: &mut Criterion) {
    let dataset = load_jsonld();
    let toon = serde_toon::to_string(&dataset).expect("encode failed");
    let json = serde_json::to_string(&dataset).expect("json encode failed");

    let mut group = c.benchmark_group("peanuts_jsonld");
    group.bench_function("encode_toon", |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(&dataset)).expect("encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_toon", |b| {
        b.iter(|| {
            let decoded: JsonLdDataset =
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
            let decoded: JsonLdDataset =
                serde_json::from_str(black_box(&json)).expect("json decode failed");
            black_box(decoded);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_peanuts_jsonld);
criterion_main!(benches);
