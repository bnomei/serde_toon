use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};

const PEANUTS_UNIVERSE_JSON: &[u8] = include_bytes!("../benchmarks/data/peanuts_universe.json");

#[derive(Clone, Serialize, Deserialize)]
struct UniverseDates {
    daily: String,
    sunday: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct UniverseCounts {
    #[serde(rename = "stripCount")]
    strip_count: u32,
    #[serde(rename = "peakNewspapers")]
    peak_newspapers: u32,
    countries: u32,
    languages: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct UniverseContext {
    title: String,
    author: String,
    launch: UniverseDates,
    end: UniverseDates,
    #[serde(rename = "currentStatus")]
    current_status: String,
    website: String,
    genres: Vec<String>,
    #[serde(rename = "notableCounts")]
    notable_counts: UniverseCounts,
}

#[derive(Clone, Serialize, Deserialize)]
struct Owner {
    name: String,
    #[serde(rename = "startYear")]
    start_year: u16,
    #[serde(rename = "endYear")]
    end_year: Option<u16>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Syndicate {
    name: String,
    #[serde(rename = "startYear")]
    start_year: u16,
    #[serde(rename = "endYear")]
    end_year: Option<u16>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Character {
    id: u64,
    slug: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "introducedYear")]
    introduced_year: u16,
    species: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Special {
    id: u64,
    slug: String,
    title: String,
    #[serde(rename = "airDate")]
    air_date: String,
    network: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Relationship {
    source: String,
    relation: String,
    target: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct UniverseDataset {
    context: UniverseContext,
    owners: Vec<Owner>,
    syndicates: Vec<Syndicate>,
    characters: Vec<Character>,
    specials: Vec<Special>,
    relationships: Vec<Relationship>,
    sources: Vec<String>,
}

fn load_universe() -> UniverseDataset {
    serde_json::from_slice(PEANUTS_UNIVERSE_JSON)
        .expect("failed to parse benchmarks/data/peanuts_universe.json")
}

fn bench_peanuts_universe(c: &mut Criterion) {
    let dataset = load_universe();
    let toon = serde_toon::to_string(&dataset).expect("encode failed");
    let json = serde_json::to_string(&dataset).expect("json encode failed");

    let mut group = c.benchmark_group("peanuts_universe");
    group.bench_function("encode_toon", |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(&dataset)).expect("encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_toon", |b| {
        b.iter(|| {
            let decoded: UniverseDataset =
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
            let decoded: UniverseDataset =
                serde_json::from_str(black_box(&json)).expect("json decode failed");
            black_box(decoded);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_peanuts_universe);
criterion_main!(benches);
