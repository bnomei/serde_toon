use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};

const PEANUTS_CHARACTERS_JSON: &[u8] = include_bytes!("../benchmarks/data/peanuts_characters.json");

#[derive(Clone, Serialize, Deserialize)]
struct CharactersContext {
    dataset: String,
    focus: String,
    source: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Character {
    id: u64,
    slug: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "introducedYear")]
    introduced_year: u32,
    #[serde(rename = "lastAppearanceYear")]
    last_appearance_year: u32,
    species: String,
    role: String,
    traits: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct CharactersDataset {
    context: CharactersContext,
    sources: Vec<String>,
    characters: Vec<Character>,
}

fn load_characters() -> CharactersDataset {
    serde_json::from_slice(PEANUTS_CHARACTERS_JSON)
        .expect("failed to parse benchmarks/data/peanuts_characters.json")
}

fn bench_peanuts_characters(c: &mut Criterion) {
    let dataset = load_characters();
    let toon = serde_toon::to_string(&dataset).expect("encode failed");
    let json = serde_json::to_string(&dataset).expect("json encode failed");

    let mut group = c.benchmark_group("peanuts_characters");
    group.bench_function("encode_toon", |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(&dataset)).expect("encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_toon", |b| {
        b.iter(|| {
            let decoded: CharactersDataset =
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
            let decoded: CharactersDataset =
                serde_json::from_str(black_box(&json)).expect("json decode failed");
            black_box(decoded);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_peanuts_characters);
criterion_main!(benches);
