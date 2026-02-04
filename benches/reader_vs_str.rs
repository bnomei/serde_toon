use std::io::Cursor;

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use serde::{Deserialize, Serialize};

const GITHUB_REPOS_JSON: &[u8] = include_bytes!("../benchmarks/data/github-repos.json");

#[derive(Clone, Serialize, Deserialize)]
struct GitHubRepo {
    id: u64,
    name: String,
    repo: String,
    description: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "pushedAt")]
    pushed_at: String,
    stars: u64,
    watchers: u64,
    forks: u64,
    #[serde(rename = "defaultBranch")]
    default_branch: String,
}

fn load_github_repos() -> Vec<GitHubRepo> {
    serde_json::from_slice(GITHUB_REPOS_JSON)
        .expect("failed to parse benchmarks/data/github-repos.json")
}

fn bench_reader_vs_str(c: &mut Criterion) {
    let repos = load_github_repos();
    let toon = serde_toon::to_string(&repos).expect("encode failed");
    let toon_bytes = toon.as_bytes();
    let toon_len = toon_bytes.len() as u64;
    assert!(toon_len > 0, "expected non-empty TOON payload");

    let mut group = c.benchmark_group("reader_vs_str");
    group.throughput(Throughput::Bytes(toon_len));
    group.bench_function("reader_vs_str/from_str", |b| {
        b.iter(|| {
            let decoded: Vec<GitHubRepo> =
                serde_toon::from_str(black_box(&toon)).expect("decode failed");
            black_box(decoded);
        });
    });
    group.bench_function("reader_vs_str/from_reader", |b| {
        b.iter(|| {
            let cursor = Cursor::new(black_box(toon_bytes));
            let decoded: Vec<GitHubRepo> = serde_toon::from_reader(cursor).expect("decode failed");
            black_box(decoded);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_reader_vs_str);
criterion_main!(benches);
