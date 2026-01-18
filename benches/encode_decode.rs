use std::collections::BTreeMap;
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkGroup, BenchmarkId, Criterion,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const GITHUB_REPOS_JSON: &[u8] = include_bytes!("../benchmarks/data/github-repos.json");

#[derive(Clone, Serialize, Deserialize)]
struct Owner {
    id: u64,
    login: String,
    site_admin: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct License {
    key: String,
    name: String,
    spdx_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Permissions {
    admin: bool,
    push: bool,
    pull: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Repo {
    id: u64,
    name: String,
    full_name: String,
    description: Option<String>,
    private: bool,
    fork: bool,
    language: Option<String>,
    stargazers_count: u32,
    watchers_count: u32,
    forks_count: u32,
    open_issues_count: u32,
    topics: Vec<String>,
    owner: Owner,
    license: Option<License>,
    permissions: Permissions,
    archived: bool,
    disabled: bool,
}

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

#[derive(Clone, Serialize, Deserialize)]
struct Node {
    name: String,
    value: i64,
    flags: Vec<String>,
    children: Vec<Node>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Meta {
    score: f64,
    note: String,
    flags: Vec<bool>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Row {
    Basic {
        id: u64,
        name: String,
        active: bool,
    },
    Extended {
        id: u64,
        name: String,
        active: bool,
        meta: Meta,
        tags: Vec<String>,
    },
    Minimal {
        id: u64,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Setting {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<String>),
    Section(BTreeMap<String, Setting>),
}

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    name: String,
    version: u32,
    settings: BTreeMap<String, Setting>,
}

fn load_github_repos() -> Vec<GitHubRepo> {
    serde_json::from_slice(GITHUB_REPOS_JSON).unwrap()
}

fn make_uniform_repos(count: usize) -> Vec<Repo> {
    let mut repos = Vec::with_capacity(count);
    for i in 0..count {
        let topics = vec![
            format!("topic-{}", i % 10),
            format!("topic-{}", (i + 3) % 10),
            format!("topic-{}", (i + 7) % 10),
        ];
        let owner = Owner {
            id: (i % 100) as u64,
            login: format!("user-{}", i % 100),
            site_admin: i % 97 == 0,
        };
        let license = if i % 4 == 0 {
            Some(License {
                key: "mit".to_string(),
                name: "MIT License".to_string(),
                spdx_id: Some("MIT".to_string()),
            })
        } else {
            None
        };
        let permissions = Permissions {
            admin: i % 13 == 0,
            push: i % 3 == 0,
            pull: true,
        };
        repos.push(Repo {
            id: i as u64,
            name: format!("repo-{}", i),
            full_name: format!("org/repo-{}", i),
            description: if i % 3 == 0 {
                None
            } else {
                Some(format!("Repository {}", i))
            },
            private: i % 10 == 0,
            fork: i % 7 == 0,
            language: match i % 5 {
                0 => Some("Rust".to_string()),
                1 => Some("Go".to_string()),
                2 => Some("TypeScript".to_string()),
                3 => Some("Python".to_string()),
                _ => None,
            },
            stargazers_count: (i * 13) as u32,
            watchers_count: (i * 9) as u32,
            forks_count: (i * 3) as u32,
            open_issues_count: (i * 2) as u32,
            topics,
            owner,
            license,
            permissions,
            archived: i % 37 == 0,
            disabled: i % 53 == 0,
        });
    }
    repos
}

fn make_tree(depth: usize, width: usize, seed: u64) -> Node {
    let mut children = Vec::new();
    if depth > 0 {
        for i in 0..width {
            children.push(make_tree(depth - 1, width, seed * 31 + i as u64));
        }
    }
    let flags = vec![
        format!("f{}", seed % 5),
        format!("f{}", (seed + 2) % 5),
        format!("f{}", (seed + 4) % 5),
    ];
    Node {
        name: format!("node-{}", seed),
        value: seed as i64 - 500,
        flags,
        children,
    }
}

fn make_semi_uniform_rows(count: usize) -> Vec<Row> {
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let id = i as u64;
        if i % 10 == 0 {
            rows.push(Row::Minimal { id });
        } else if i % 3 == 0 {
            rows.push(Row::Extended {
                id,
                name: format!("row-{}", i),
                active: i % 2 == 0,
                meta: Meta {
                    score: (i as f64) * 0.75,
                    note: format!("note-{}", i % 7),
                    flags: vec![i % 2 == 0, i % 3 == 0, i % 5 == 0],
                },
                tags: vec![format!("tag-{}", i % 5), format!("tag-{}", (i + 2) % 5)],
            });
        } else {
            rows.push(Row::Basic {
                id,
                name: format!("row-{}", i),
                active: i % 2 == 0,
            });
        }
    }
    rows
}

fn make_settings(depth: usize, breadth: usize, seed: u64) -> BTreeMap<String, Setting> {
    let mut map = BTreeMap::new();
    for i in 0..breadth {
        let key = format!("k{}_{}", seed, i);
        let setting = if depth == 0 {
            match i % 5 {
                0 => Setting::Bool(i % 2 == 0),
                1 => Setting::Int((seed as i64) * 10 + i as i64),
                2 => Setting::Float((seed as f64) * 0.1 + i as f64),
                3 => Setting::Text(format!("value-{}", seed + i as u64)),
                _ => Setting::List(vec![
                    format!("item-{}", i),
                    format!("item-{}", i + 1),
                    format!("item-{}", i + 2),
                ]),
            }
        } else if i % 4 == 0 {
            Setting::Section(make_settings(depth - 1, breadth, seed * 7 + i as u64))
        } else {
            Setting::Text(format!("leaf-{}-{}", seed, i))
        };
        map.insert(key, setting);
    }
    map
}

fn make_config(depth: usize, breadth: usize) -> Config {
    Config {
        name: "app-config".to_string(),
        version: 3,
        settings: make_settings(depth, breadth, 1),
    }
}

fn bench_encode<T: Serialize>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    value: &T,
    toon_len: usize,
    json_len: usize,
) {
    group.throughput(criterion::Throughput::Bytes(toon_len as u64));
    group.bench_function(BenchmarkId::new("toon", name), |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(value)).unwrap();
            black_box(encoded);
        });
    });

    group.throughput(criterion::Throughput::Bytes(json_len as u64));
    group.bench_function(BenchmarkId::new("json", name), |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(black_box(value)).unwrap();
            black_box(encoded);
        });
    });
}

fn bench_decode<T: DeserializeOwned>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    toon_text: &str,
    json_text: &str,
) {
    group.throughput(criterion::Throughput::Bytes(toon_text.len() as u64));
    group.bench_function(BenchmarkId::new("toon", name), |b| {
        b.iter(|| {
            let value: T = serde_toon::from_str(black_box(toon_text)).unwrap();
            black_box(value);
        });
    });

    group.throughput(criterion::Throughput::Bytes(json_text.len() as u64));
    group.bench_function(BenchmarkId::new("json", name), |b| {
        b.iter(|| {
            let value: T = serde_json::from_str(black_box(json_text)).unwrap();
            black_box(value);
        });
    });
}

fn bench_roundtrip<T: Serialize + DeserializeOwned>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    value: &T,
) {
    group.bench_function(BenchmarkId::new("toon", name), |b| {
        b.iter(|| {
            let encoded = serde_toon::to_string(black_box(value)).unwrap();
            let round: T = serde_toon::from_str(&encoded).unwrap();
            black_box(round);
        });
    });

    group.bench_function(BenchmarkId::new("json", name), |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(black_box(value)).unwrap();
            let round: T = serde_json::from_str(&encoded).unwrap();
            black_box(round);
        });
    });
}

fn quick_encode<T: Serialize>(value: &T) {
    let encoded = serde_toon::to_string(black_box(value)).unwrap();
    black_box(encoded);

    let encoded = serde_json::to_string(black_box(value)).unwrap();
    black_box(encoded);
}

fn quick_decode<T: DeserializeOwned>(toon_text: &str, json_text: &str) {
    let value: T = serde_toon::from_str(black_box(toon_text)).unwrap();
    black_box(value);

    let value: T = serde_json::from_str(black_box(json_text)).unwrap();
    black_box(value);
}

fn quick_roundtrip<T: Serialize + DeserializeOwned>(value: &T) {
    let encoded = serde_toon::to_string(black_box(value)).unwrap();
    let round: T = serde_toon::from_str(&encoded).unwrap();
    black_box(round);

    let encoded = serde_json::to_string(black_box(value)).unwrap();
    let round: T = serde_json::from_str(&encoded).unwrap();
    black_box(round);
}

fn criterion_config() -> Criterion {
    if std::env::var("TOON_BENCH_MINIMAL").is_ok() {
        Criterion::default()
            .warm_up_time(Duration::from_secs(0))
            .measurement_time(Duration::from_millis(10))
            .sample_size(1)
            .nresamples(1)
    } else {
        Criterion::default()
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let uniform = make_uniform_repos(2000);
    let uniform_toon = serde_toon::to_string(&uniform).unwrap();
    let uniform_json = serde_json::to_string(&uniform).unwrap();

    let deep_tree = make_tree(5, 3, 1);
    let tree_toon = serde_toon::to_string(&deep_tree).unwrap();
    let tree_json = serde_json::to_string(&deep_tree).unwrap();

    let semi_uniform = make_semi_uniform_rows(2500);
    let semi_toon = serde_toon::to_string(&semi_uniform).unwrap();
    let semi_json = serde_json::to_string(&semi_uniform).unwrap();

    let config = make_config(3, 6);
    let config_toon = serde_toon::to_string(&config).unwrap();
    let config_json = serde_json::to_string(&config).unwrap();

    let github_repos = load_github_repos();
    let github_toon = serde_toon::to_string(&github_repos).unwrap();
    let github_json = serde_json::to_string(&github_repos).unwrap();

    if std::env::var("TOON_BENCH_QUICK").is_ok() {
        quick_encode(&uniform);
        quick_encode(&deep_tree);
        quick_encode(&semi_uniform);
        quick_encode(&config);
        quick_encode(&github_repos);

        quick_decode::<Vec<Repo>>(&uniform_toon, &uniform_json);
        quick_decode::<Node>(&tree_toon, &tree_json);
        quick_decode::<Vec<Row>>(&semi_toon, &semi_json);
        quick_decode::<Config>(&config_toon, &config_json);
        quick_decode::<Vec<GitHubRepo>>(&github_toon, &github_json);

        quick_roundtrip(&uniform);
        quick_roundtrip(&deep_tree);
        quick_roundtrip(&semi_uniform);
        quick_roundtrip(&config);
        quick_roundtrip(&github_repos);
        return;
    }

    let mut encode = c.benchmark_group("encode");
    bench_encode(
        &mut encode,
        "uniform_repos",
        &uniform,
        uniform_toon.len(),
        uniform_json.len(),
    );
    bench_encode(
        &mut encode,
        "deep_tree",
        &deep_tree,
        tree_toon.len(),
        tree_json.len(),
    );
    bench_encode(
        &mut encode,
        "semi_uniform_rows",
        &semi_uniform,
        semi_toon.len(),
        semi_json.len(),
    );
    bench_encode(
        &mut encode,
        "config_map",
        &config,
        config_toon.len(),
        config_json.len(),
    );
    bench_encode(
        &mut encode,
        "github_repos",
        &github_repos,
        github_toon.len(),
        github_json.len(),
    );
    encode.finish();

    let mut decode = c.benchmark_group("decode");
    bench_decode::<Vec<Repo>>(&mut decode, "uniform_repos", &uniform_toon, &uniform_json);
    bench_decode::<Node>(&mut decode, "deep_tree", &tree_toon, &tree_json);
    bench_decode::<Vec<Row>>(&mut decode, "semi_uniform_rows", &semi_toon, &semi_json);
    bench_decode::<Config>(&mut decode, "config_map", &config_toon, &config_json);
    bench_decode::<Vec<GitHubRepo>>(&mut decode, "github_repos", &github_toon, &github_json);
    decode.finish();

    let mut roundtrip = c.benchmark_group("roundtrip");
    bench_roundtrip(&mut roundtrip, "uniform_repos", &uniform);
    bench_roundtrip(&mut roundtrip, "deep_tree", &deep_tree);
    bench_roundtrip(&mut roundtrip, "semi_uniform_rows", &semi_uniform);
    bench_roundtrip(&mut roundtrip, "config_map", &config);
    bench_roundtrip(&mut roundtrip, "github_repos", &github_repos);
    roundtrip.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = criterion_benchmark
}
criterion_main!(benches);
