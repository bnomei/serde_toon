use std::convert::TryFrom;
use std::env;
use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pprof::ProfilerGuard;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

fn parse_args() -> (u64, PathBuf, u32, Option<PathBuf>, bool) {
    let mut seconds = 30_u64;
    let mut out_prefix = PathBuf::from("benchmarks/profiles/encode_github");
    let mut frequency = 100_u32;
    let mut input: Option<PathBuf> = None;
    let mut use_bytes = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| "--seconds requires a value".to_string());
                seconds = value.parse().expect("invalid --seconds value");
            }
            "--out" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| "--out requires a value".to_string());
                out_prefix = PathBuf::from(value);
                if out_prefix.extension().is_some() {
                    out_prefix = out_prefix.with_extension("");
                }
            }
            "--freq" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| "--freq requires a value".to_string());
                frequency = value.parse().expect("invalid --freq value");
            }
            "--input" => {
                let value = args
                    .next()
                    .unwrap_or_else(|| "--input requires a value".to_string());
                input = Some(PathBuf::from(value));
            }
            "--bytes" => {
                use_bytes = true;
            }
            _ => {
                eprintln!("unknown arg: {arg}");
            }
        }
    }

    (seconds, out_prefix, frequency, input, use_bytes)
}

fn main() {
    let (seconds, out_prefix, frequency, input, use_bytes) = parse_args();
    let json_value: Option<Value> = input.as_ref().map(|path| {
        let bytes = fs::read(path).expect("failed to read --input file");
        serde_json::from_slice(&bytes).expect("failed to parse --input JSON")
    });
    let repos: Option<Vec<GitHubRepo>> = if json_value.is_some() {
        None
    } else {
        Some(
            serde_json::from_slice(GITHUB_REPOS_JSON)
                .expect("failed to parse benchmarks/data/github-repos.json"),
        )
    };

    let guard = ProfilerGuard::new(i32::try_from(frequency).expect("invalid --freq value"))
        .expect("failed to start profiler");
    let start = Instant::now();
    let deadline = Duration::from_secs(seconds);
    let mut iterations = 0_u64;

    while start.elapsed() < deadline {
        if use_bytes {
            let encoded = if let Some(repos) = &repos {
                serde_toon::to_vec(repos).expect("encode failed")
            } else {
                serde_toon::to_vec(json_value.as_ref().expect("missing JSON value for encode"))
                    .expect("encode failed")
            };
            std::hint::black_box(encoded);
        } else {
            let encoded = if let Some(repos) = &repos {
                serde_toon::to_string(repos).expect("encode failed")
            } else {
                serde_toon::to_string(json_value.as_ref().expect("missing JSON value for encode"))
                    .expect("encode failed")
            };
            std::hint::black_box(encoded);
        }
        iterations += 1;
    }

    eprintln!("iterations: {iterations}");

    if let Ok(report) = guard.report().build() {
        if let Some(parent) = out_prefix.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).expect("failed to create output dir");
            }
        }
        let svg_path = out_prefix.with_extension("svg");
        let mut svg = File::create(&svg_path).expect("failed to create svg output");
        report
            .flamegraph(&mut svg)
            .expect("failed to write flamegraph");
        eprintln!("wrote {}", svg_path.display());
    }
}
