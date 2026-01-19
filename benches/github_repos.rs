use criterion::{black_box, criterion_group, criterion_main, Criterion};
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

fn bench_github_repos(c: &mut Criterion) {
    let repos = load_github_repos();
    let toon = serde_toon::to_string(&repos).expect("encode failed");
    let json = serde_json::to_string(&repos).expect("json encode failed");

    let mut group = c.benchmark_group("github_repos");
    group.bench_function("encode_toon", |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(&repos)).expect("encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_toon", |b| {
        b.iter(|| {
            let decoded: Vec<GitHubRepo> =
                serde_toon::from_str(black_box(&toon)).expect("decode failed");
            black_box(decoded);
        });
    });
    group.bench_function("encode_json", |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(black_box(&repos)).expect("json encode failed");
            black_box(encoded);
        });
    });
    group.bench_function("decode_json", |b| {
        b.iter(|| {
            let decoded: Vec<GitHubRepo> =
                serde_json::from_str(black_box(&json)).expect("json decode failed");
            black_box(decoded);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_github_repos);
criterion_main!(benches);
