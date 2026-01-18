use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct User {
    name: String,
    age: u32,
    active: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Team {
    id: u64,
    name: String,
    users: Vec<User>,
}

fn make_users(count: usize) -> Vec<User> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(User {
            name: format!("user-{}", i),
            age: (i % 100) as u32,
            active: i % 2 == 0,
        });
    }
    out
}

fn make_team(count: usize) -> Team {
    Team {
        id: 42,
        name: "ops".to_string(),
        users: make_users(count),
    }
}

fn bench_encode_decode(c: &mut Criterion) {
    if std::env::var("TOON_BENCH_QUICK").is_ok() {
        for size in [10_usize, 100, 1000] {
            let team = make_team(size);
            let encoded = serde_toon::to_string(black_box(&team)).unwrap();
            black_box(encoded);

            let encoded = serde_toon::to_string(&team).unwrap();
            let decoded: Team = serde_toon::from_str(black_box(&encoded)).unwrap();
            black_box(decoded);
        }

        for size in [100_usize, 1000, 5000] {
            let users = make_users(size);
            let encoded = serde_toon::to_string(black_box(&users)).unwrap();
            black_box(encoded);

            let encoded = serde_toon::to_string(&users).unwrap();
            let decoded: Vec<User> = serde_toon::from_str(black_box(&encoded)).unwrap();
            black_box(decoded);
        }
        return;
    }

    let mut group = c.benchmark_group("src2_canonical");
    for size in [10_usize, 100, 1000] {
        let team = make_team(size);
        group.bench_with_input(BenchmarkId::new("encode", size), &team, |b, data| {
            b.iter(|| {
                let encoded = serde_toon::to_string(black_box(data)).unwrap();
                black_box(encoded);
            })
        });

        let encoded = serde_toon::to_string(&team).unwrap();
        group.bench_with_input(BenchmarkId::new("decode", size), &encoded, |b, data| {
            b.iter(|| {
                let decoded: Team = serde_toon::from_str(black_box(data)).unwrap();
                black_box(decoded);
            })
        });
    }

    for size in [100_usize, 1000, 5000] {
        let users = make_users(size);
        group.bench_with_input(
            BenchmarkId::new("encode_root_array", size),
            &users,
            |b, data| {
                b.iter(|| {
                    let encoded = serde_toon::to_string(black_box(data)).unwrap();
                    black_box(encoded);
                })
            },
        );

        let encoded = serde_toon::to_string(&users).unwrap();
        group.bench_with_input(
            BenchmarkId::new("decode_root_array", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let decoded: Vec<User> = serde_toon::from_str(black_box(data)).unwrap();
                    black_box(decoded);
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_encode_decode);
criterion_main!(benches);
