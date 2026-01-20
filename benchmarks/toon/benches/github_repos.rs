use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};

const GITHUB_REPOS_JSON: &[u8] = include_bytes!("../../data/github-repos.json");

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

fn sanitize_pipe(value: &str) -> String {
    value.replace('|', "\\u007C")
}

fn sanitize_repo_for_toon_rust(repo: &GitHubRepo) -> GitHubRepo {
    GitHubRepo {
        id: repo.id,
        name: sanitize_pipe(&repo.name),
        repo: sanitize_pipe(&repo.repo),
        description: repo.description.as_deref().map(sanitize_pipe),
        created_at: sanitize_pipe(&repo.created_at),
        updated_at: sanitize_pipe(&repo.updated_at),
        pushed_at: sanitize_pipe(&repo.pushed_at),
        stars: repo.stars,
        watchers: repo.watchers,
        forks: repo.forks,
        default_branch: sanitize_pipe(&repo.default_branch),
    }
}

fn report_decode_failure(group: &str, name: &str, ok: bool) {
    if !ok {
        eprintln!("decode failed ({group}): {name}");
    }
}

fn report_decode_error(group: &str, name: &str, err: &str) {
    eprintln!("decode error ({group}): {name}: {err}");
}

fn report_patch(group: &str, name: &str, note: &str) {
    eprintln!("patched ({group}): {name} ({note})");
}

fn is_numeric_token(token: &str) -> bool {
    token.parse::<i64>().is_ok() || token.parse::<f64>().is_ok()
}

fn is_token_boundary(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t' | '\n' | '\r' | ':' | ',' | '|' | '[' | ']' | '{' | '}'
    )
}

fn quote_unquoted_tokens(input: &str, should_quote: &dyn Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_quotes = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_quotes {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            }
            continue;
        }

        if ch == '"' {
            in_quotes = true;
            out.push(ch);
            continue;
        }

        if is_token_boundary(ch) {
            out.push(ch);
            continue;
        }

        let mut token = String::new();
        token.push(ch);
        while let Some(&next) = chars.peek() {
            if next == '"' || is_token_boundary(next) {
                break;
            }
            token.push(next);
            chars.next();
        }

        if should_quote(&token) {
            out.push('"');
            for t in token.chars() {
                match t {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    _ => out.push(t),
                }
            }
            out.push('"');
        } else {
            out.push_str(&token);
        }
    }

    out
}

fn find_unquoted_colon(line: &str) -> Option<usize> {
    let mut in_quotes = false;
    let mut escaped = false;

    for (idx, ch) in line.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            }
            continue;
        }

        match ch {
            '"' => in_quotes = true,
            ':' => return Some(idx),
            _ => {}
        }
    }

    None
}

fn count_leading_spaces(line: &str) -> usize {
    line.chars().take_while(|ch| *ch == ' ').count()
}

fn is_table_header(line: &str) -> bool {
    let line = line.trim_start();
    if !line.starts_with('[') {
        return false;
    }
    let close_bracket = match line.find(']') {
        Some(idx) => idx,
        None => return false,
    };
    let after_bracket = &line[close_bracket + 1..];
    let open_brace = match after_bracket.find('{') {
        Some(idx) => idx,
        None => return false,
    };
    let close_brace = match after_bracket[open_brace + 1..].find('}') {
        Some(idx) => open_brace + 1 + idx,
        None => return false,
    };
    let after_brace = &after_bracket[close_brace + 1..];
    after_brace.trim_start().starts_with(':')
}

fn patch_values_only(input: &str, should_quote: &dyn Fn(&str) -> bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_table = false;
    let mut table_indent = None;

    for line in input.split_inclusive('\n') {
        let (line_body, line_end) = if line.ends_with('\n') {
            (&line[..line.len() - 1], "\n")
        } else {
            (line, "")
        };

        if in_table {
            if line_body.trim().is_empty() {
                out.push_str(line_body);
                out.push_str(line_end);
                continue;
            }

            let indent = count_leading_spaces(line_body);
            match table_indent {
                Some(row_indent) if indent < row_indent => {
                    in_table = false;
                    table_indent = None;
                }
                Some(_) | None => {
                    if table_indent.is_none() {
                        table_indent = Some(indent);
                    }
                    let (indent_str, rest) = line_body.split_at(indent);
                    out.push_str(indent_str);
                    out.push_str(&quote_unquoted_tokens(rest, should_quote));
                    out.push_str(line_end);
                    continue;
                }
            }
        }

        if is_table_header(line_body) {
            in_table = true;
            table_indent = None;
            out.push_str(line_body);
            out.push_str(line_end);
            continue;
        }

        if let Some(idx) = find_unquoted_colon(line_body) {
            let (head, tail) = line_body.split_at(idx + 1);
            out.push_str(head);
            out.push_str(&quote_unquoted_tokens(tail, should_quote));
        } else {
            out.push_str(&quote_unquoted_tokens(line_body, should_quote));
        }
        out.push_str(line_end);
    }

    out
}

fn should_quote_digit_leading(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token == "true" || token == "false" || token == "null" {
        return false;
    }
    if is_numeric_token(token) {
        return false;
    }

    let mut chars = token.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_digit() => true,
        Some('-') => matches!(chars.next(), Some(next) if next.is_ascii_digit()),
        _ => false,
    }
}

fn should_quote_serde_toon_v2(token: &str) -> bool {
    if should_quote_digit_leading(token) {
        return true;
    }
    match token.chars().next() {
        Some('t' | 'f' | 'n') => {
            token != "true" && token != "false" && token != "null"
        }
        _ => false,
    }
}

fn bench_github_repos(c: &mut Criterion) {
    let repos = load_github_repos();
    let repos_value = serde_json::to_value(&repos).expect("failed to convert to Value");
    let toon_rust_repos: Vec<GitHubRepo> =
        repos.iter().map(sanitize_repo_for_toon_rust).collect();
    let toon_rust_repos_value =
        serde_json::to_value(&toon_rust_repos).expect("failed to convert toon_rust Value");

    let toon_rs_opts = toon_rs::Options::default();
    let toon_rust_encode_opts = toon_rust::EncodeOptions::new();
    let rtoon_encode_opts = rtoon::EncodeOptions::new();
    let json2toon_encode_opts = json2toon_rs::EncoderOptions::default();

    let mut toon_format_decode_opts_value = toon_format::DecodeOptions::default();
    let mut toon_rust_decode_opts_value = toon_rust::DecodeOptions::new();
    let mut rtoon_decode_opts_value = rtoon::DecodeOptions::new();
    let mut json2toon_decode_opts_value = json2toon_rs::DecoderOptions::default();

    let serde_toon_v3_value = serde_toon_v3::to_string(&repos_value).ok();
    let serde_toon_v2_value = serde_toon_v2::to_string(&repos_value).ok();
    let toon_format_v2_value = toon_format::encode_default(&repos_value).ok();
    let toon_rs_value = toon_rs::encode_to_string(&repos_value, &toon_rs_opts).ok();
    let toon_rust_value =
        toon_rust::encode(&toon_rust_repos_value, Some(&toon_rust_encode_opts)).ok();
    let rtoon_value = rtoon::encode(&repos_value, &rtoon_encode_opts).ok();
    let json2toon_value = json2toon_rs::encode(&repos_value, &json2toon_encode_opts);
    let json_value = serde_json::to_string(&repos_value).expect("json encode failed");

    let serde_toon_v3_value_encode_ok = serde_toon_v3_value.is_some();
    let serde_toon_v2_value_encode_ok = serde_toon_v2_value.is_some();
    let toon_format_v2_value_encode_ok = toon_format_v2_value.is_some();
    let toon_rs_value_encode_ok = toon_rs_value.is_some();
    let toon_rust_value_encode_ok = toon_rust_value.is_some();
    let rtoon_value_encode_ok = rtoon_value.is_some();

    let serde_toon_v3_value_ok = serde_toon_v3_value
        .as_deref()
        .map_or(false, |encoded| {
            serde_toon_v3::from_str::<serde_json::Value>(encoded).is_ok()
        });
    let mut serde_toon_v2_decode_via_value = false;
    let mut serde_toon_v2_value_fixed: Option<String> = None;
    let mut serde_toon_v2_value_patched = false;
    let mut serde_toon_v2_value_ok = serde_toon_v2_value
        .as_deref()
        .map_or(false, |encoded| {
            serde_toon_v2::from_str::<serde_json::Value>(encoded).is_ok()
        });
    if !serde_toon_v2_value_ok {
        let patched_ok = serde_toon_v2_value
            .as_deref()
            .and_then(|encoded| serde_toon_v2::from_str::<serde_toon_v2::Value>(encoded).ok())
            .and_then(|value| serde_json::to_value(value).ok())
            .is_some();
        if patched_ok {
            serde_toon_v2_value_ok = true;
            serde_toon_v2_decode_via_value = true;
        }
    }
    if !serde_toon_v2_value_ok {
        if let Some(encoded) = serde_toon_v2_value.as_deref() {
            let patched = patch_values_only(encoded, &should_quote_serde_toon_v2);
            let patched_json_ok =
                serde_toon_v2::from_str::<serde_json::Value>(&patched).is_ok();
            let patched_value_ok = serde_toon_v2::from_str::<serde_toon_v2::Value>(&patched)
                .ok()
                .and_then(|value| serde_json::to_value(value).ok())
                .is_some();
            if patched_json_ok || patched_value_ok {
                serde_toon_v2_value_fixed = Some(patched);
                serde_toon_v2_value_ok = true;
                serde_toon_v2_value_patched = true;
                serde_toon_v2_decode_via_value = !patched_json_ok;
            }
        }
    }
    let mut toon_format_v2_value_ok = toon_format_v2_value
        .as_deref()
        .map_or(false, |encoded| {
            toon_format::decode::<serde_json::Value>(encoded, &toon_format_decode_opts_value)
                .is_ok()
        });
    let toon_rs_value_ok = toon_rs_value
        .as_deref()
        .map_or(false, |encoded| {
            toon_rs::decode_from_str::<serde_json::Value>(encoded, &toon_rs_opts)
                .is_ok()
        });
    let mut toon_rust_value_ok = toon_rust_value
        .as_deref()
        .map_or(false, |encoded| {
            toon_rust::decode(encoded, Some(&toon_rust_decode_opts_value)).is_ok()
        });
    let mut rtoon_value_ok = rtoon_value
        .as_deref()
        .map_or(false, |encoded| rtoon::decode(encoded, &rtoon_decode_opts_value).is_ok());
    let mut json2toon_value_ok =
        json2toon_rs::decode(&json2toon_value, &json2toon_decode_opts_value).is_ok();
    let mut toon_format_value_patched = false;
    if !toon_format_v2_value_ok {
        let patched_opts = toon_format::DecodeOptions::new()
            .with_strict(false)
            .with_coerce_types(false);
        let patched_ok = toon_format_v2_value
            .as_deref()
            .map_or(false, |encoded| {
                toon_format::decode::<serde_json::Value>(encoded, &patched_opts).is_ok()
            });
        if patched_ok {
            toon_format_decode_opts_value = patched_opts;
            toon_format_v2_value_ok = true;
            toon_format_value_patched = true;
        }
    }

    let mut toon_rust_value_patched = false;
    if !toon_rust_value_ok {
        let patched_opts = toon_rust::DecodeOptions::new().strict(false);
        let patched_ok = toon_rust_value
            .as_deref()
            .map_or(false, |encoded| {
                toon_rust::decode(encoded, Some(&patched_opts)).is_ok()
            });
        if patched_ok {
            toon_rust_decode_opts_value = patched_opts;
            toon_rust_value_ok = true;
            toon_rust_value_patched = true;
        }
    }

    let mut rtoon_value_fixed: Option<String> = None;
    let mut rtoon_value_delimiter_patched = false;
    let mut rtoon_value_strict_patched = false;
    let mut rtoon_value_token_patched = false;
    if !rtoon_value_ok {
        let patched_opts = rtoon::DecodeOptions::new().with_delimiter(rtoon::Delimiter::Comma);
        let patched_ok = rtoon_value
            .as_deref()
            .map_or(false, |encoded| rtoon::decode(encoded, &patched_opts).is_ok());
        if patched_ok {
            rtoon_decode_opts_value = patched_opts;
            rtoon_value_ok = true;
            rtoon_value_delimiter_patched = true;
        }
    }

    if !rtoon_value_ok {
        let patched_opts = rtoon_decode_opts_value.clone().with_strict(false);
        let patched_ok = rtoon_value
            .as_deref()
            .map_or(false, |encoded| rtoon::decode(encoded, &patched_opts).is_ok());
        if patched_ok {
            rtoon_decode_opts_value = patched_opts;
            rtoon_value_ok = true;
            rtoon_value_strict_patched = true;
        }
    }

    if !rtoon_value_ok {
        if let Some(encoded) = rtoon_value.as_deref() {
            let patched = patch_values_only(encoded, &should_quote_digit_leading);
            let patched_ok = rtoon::decode(&patched, &rtoon_decode_opts_value).is_ok();
            if patched_ok {
                rtoon_value_fixed = Some(patched);
                rtoon_value_ok = true;
                rtoon_value_token_patched = true;
            }
        }
    }

    let mut json2toon_value_patched = false;
    if !json2toon_value_ok {
        let mut patched_opts = json2toon_rs::DecoderOptions::default();
        patched_opts.strict = false;
        let patched_ok =
            json2toon_rs::decode(&json2toon_value, &patched_opts).is_ok();
        if patched_ok {
            json2toon_decode_opts_value = patched_opts;
            json2toon_value_ok = true;
            json2toon_value_patched = true;
        }
    }

    let json_value_ok =
        serde_json::from_str::<serde_json::Value>(&json_value).is_ok();
    let serde_toon_v2_value_for_decode =
        serde_toon_v2_value_fixed.as_deref().or(serde_toon_v2_value.as_deref());
    let rtoon_value_for_decode =
        rtoon_value_fixed.as_deref().or(rtoon_value.as_deref());

    report_decode_failure("value", "serde_toon_v3", serde_toon_v3_value_ok);
    if serde_toon_v2_decode_via_value {
        report_patch("value", "serde_toon_v2", "decode via serde_toon::Value");
    }
    if serde_toon_v2_value_patched {
        report_patch("value", "serde_toon_v2", "quoted t/f/n + digit-leading strings");
    }
    report_decode_failure("value", "serde_toon_v2", serde_toon_v2_value_ok);
    if toon_format_value_patched {
        report_patch("value", "toon_format_v2", "strict=false, coerce_types=false");
    }
    report_decode_failure("value", "toon_format_v2", toon_format_v2_value_ok);
    report_decode_failure("value", "toon_rs_v3", toon_rs_value_ok);
    if toon_rust_value_encode_ok {
        report_patch("value", "toon_rust_v1", "sanitized '|' -> \\u007C");
    }
    if toon_rust_value_patched {
        report_patch("value", "toon_rust_v1", "strict=false");
    }
    report_decode_failure("value", "toon_rust_v1", toon_rust_value_ok);
    if rtoon_value_delimiter_patched {
        report_patch("value", "rtoon_v1", "delimiter=comma");
    }
    if rtoon_value_strict_patched {
        report_patch("value", "rtoon_v1", "strict=false");
    }
    if rtoon_value_token_patched {
        report_patch("value", "rtoon_v1", "quoted digit-leading strings");
    }
    report_decode_failure("value", "rtoon_v1", rtoon_value_ok);
    if json2toon_value_patched {
        report_patch("value", "json2toon_v2", "strict=false");
    }
    report_decode_failure("value", "json2toon_v2", json2toon_value_ok);
    report_decode_failure("value", "serde_json", json_value_ok);

    if !serde_toon_v2_value_ok {
        if let Some(encoded) = serde_toon_v2_value_for_decode {
            if let Err(err) = serde_toon_v2::from_str::<serde_json::Value>(encoded) {
                report_decode_error("value", "serde_toon_v2", &err.to_string());
            }
        }
    }
    if !toon_rust_value_ok {
        if let Some(encoded) = toon_rust_value.as_deref() {
            if let Err(err) =
                toon_rust::decode(encoded, Some(&toon_rust_decode_opts_value))
            {
                report_decode_error("value", "toon_rust_v1", &err.to_string());
            }
        }
    }
    if !rtoon_value_ok {
        if let Some(encoded) = rtoon_value_for_decode {
            if let Err(err) = rtoon::decode(encoded, &rtoon_decode_opts_value) {
                report_decode_error("value", "rtoon_v1", &err.to_string());
            }
        }
    }

    let mut group = c.benchmark_group("github_repos_value");

    if serde_toon_v3_value_encode_ok {
        group.bench_function("encode_serde_toon_v3", |b| {
            b.iter(|| {
                let encoded = serde_toon_v3::to_string(black_box(&repos_value))
                    .expect("serde_toon_v3 encode failed");
                black_box(encoded);
            });
        });
    }

    if serde_toon_v3_value_ok {
        let encoded = serde_toon_v3_value.as_ref().expect("encoded");
        group.bench_function("decode_serde_toon_v3", |b| {
            b.iter(|| {
                let decoded: serde_json::Value =
                    serde_toon_v3::from_str(black_box(encoded.as_str()))
                        .expect("serde_toon_v3 decode failed");
                black_box(decoded);
            });
        });
    }

    if serde_toon_v2_value_encode_ok {
        group.bench_function("encode_serde_toon_v2", |b| {
            b.iter(|| {
                let encoded = serde_toon_v2::to_string(black_box(&repos_value))
                    .expect("serde_toon_v2 encode failed");
                black_box(encoded);
            });
        });
    }

    if serde_toon_v2_value_ok {
        let encoded = serde_toon_v2_value_for_decode.expect("encoded");
        group.bench_function("decode_serde_toon_v2", |b| {
            b.iter(|| {
                if serde_toon_v2_decode_via_value {
                    let decoded: serde_toon_v2::Value =
                        serde_toon_v2::from_str(black_box(encoded))
                            .expect("serde_toon_v2 decode failed");
                    let decoded_json =
                        serde_json::to_value(decoded)
                            .expect("serde_toon_v2 to serde_json failed");
                    black_box(decoded_json);
                } else {
                    let decoded: serde_json::Value =
                        serde_toon_v2::from_str(black_box(encoded))
                            .expect("serde_toon_v2 decode failed");
                    black_box(decoded);
                }
            });
        });
    }

    if toon_format_v2_value_encode_ok {
        group.bench_function("encode_toon_format_v2", |b| {
            b.iter(|| {
                let encoded = toon_format::encode_default(black_box(&repos_value))
                    .expect("toon-format encode failed");
                black_box(encoded);
            });
        });
    }

    if toon_format_v2_value_ok {
        let encoded = toon_format_v2_value.as_ref().expect("encoded");
        group.bench_function("decode_toon_format_v2", |b| {
            b.iter(|| {
                let decoded: serde_json::Value =
                    toon_format::decode(
                        black_box(encoded.as_str()),
                        &toon_format_decode_opts_value,
                    )
                    .expect("toon-format decode failed");
                black_box(decoded);
            });
        });
    }

    if toon_rs_value_encode_ok {
        group.bench_function("encode_toon_rs_v3", |b| {
            b.iter(|| {
                let encoded =
                    toon_rs::encode_to_string(black_box(&repos_value), &toon_rs_opts)
                        .expect("toon-rs encode failed");
                black_box(encoded);
            });
        });
    }

    if toon_rs_value_ok {
        let encoded = toon_rs_value.as_ref().expect("encoded");
        group.bench_function("decode_toon_rs_v3", |b| {
            b.iter(|| {
                let decoded_value: serde_json::Value =
                    toon_rs::decode_from_str(black_box(encoded.as_str()), &toon_rs_opts)
                        .expect("toon-rs decode failed");
                black_box(decoded_value);
            });
        });
    }

    if toon_rust_value_encode_ok {
        group.bench_function("encode_toon_rust_v1", |b| {
            b.iter(|| {
                let encoded = toon_rust::encode(
                    black_box(&toon_rust_repos_value),
                    Some(&toon_rust_encode_opts),
                )
                .expect("toon-rust encode failed");
                black_box(encoded);
            });
        });
    }

    if toon_rust_value_ok {
        let encoded = toon_rust_value.as_ref().expect("encoded");
        group.bench_function("decode_toon_rust_v1", |b| {
            b.iter(|| {
                let decoded = toon_rust::decode(
                    black_box(encoded.as_str()),
                    Some(&toon_rust_decode_opts_value),
                )
                .expect("toon-rust decode failed");
                black_box(decoded);
            });
        });
    }

    if rtoon_value_encode_ok {
        group.bench_function("encode_rtoon_v1", |b| {
            b.iter(|| {
                let encoded = rtoon::encode(
                    black_box(&repos_value),
                    &rtoon_encode_opts,
                )
                .expect("rtoon encode failed");
                black_box(encoded);
            });
        });
    }

    if rtoon_value_ok {
        let encoded = rtoon_value_for_decode.expect("encoded");
        group.bench_function("decode_rtoon_v1", |b| {
            b.iter(|| {
                let decoded = rtoon::decode(
                    black_box(encoded),
                    &rtoon_decode_opts_value,
                )
                .expect("rtoon decode failed");
                black_box(decoded);
            });
        });
    }

    group.bench_function("encode_json2toon_v2", |b| {
        b.iter(|| {
            let encoded = json2toon_rs::encode(
                black_box(&repos_value),
                &json2toon_encode_opts,
            );
            black_box(encoded);
        });
    });

    if json2toon_value_ok {
        group.bench_function("decode_json2toon_v2", |b| {
            b.iter(|| {
                let decoded_value = json2toon_rs::decode(
                    black_box(json2toon_value.as_str()),
                    &json2toon_decode_opts_value,
                )
                .expect("json2toon decode failed");
                black_box(decoded_value);
            });
        });
    }

    group.bench_function("encode_toon_v1", |b| {
        b.iter(|| {
            let encoded = toon::encode(black_box(&repos_value), None);
            black_box(encoded);
        });
    });

    group.bench_function("encode_json", |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(black_box(&repos_value))
                .expect("json encode failed");
            black_box(encoded);
        });
    });

    group.bench_function("decode_json", |b| {
        b.iter(|| {
            let decoded: serde_json::Value =
                serde_json::from_str(black_box(json_value.as_str()))
                    .expect("json decode failed");
            black_box(decoded);
        });
    });

    group.finish();

    let serde_toon_v3_typed = serde_toon_v3::to_string(&repos).ok();
    let serde_toon_v2_typed = serde_toon_v2::to_string(&repos).ok();
    let toon_format_v2_typed = toon_format::encode_default(&repos).ok();
    let toon_rs_typed = toon_rs::encode_to_string(&repos, &toon_rs_opts).ok();
    let toon_rust_typed =
        toon_rust::serde_api::to_string_with_options(&toon_rust_repos, &toon_rust_encode_opts)
            .ok();
    let rtoon_typed = rtoon::to_toon(&repos, Some(&rtoon_encode_opts)).ok();
    let json_typed = serde_json::to_string(&repos).expect("json encode failed");

    let mut toon_format_decode_opts_typed = toon_format::DecodeOptions::default();
    let mut toon_rust_decode_opts_typed = toon_rust::DecodeOptions::new();
    let mut rtoon_decode_opts_typed = rtoon::DecodeOptions::new();

    let serde_toon_v3_typed_encode_ok = serde_toon_v3_typed.is_some();
    let serde_toon_v2_typed_encode_ok = serde_toon_v2_typed.is_some();
    let toon_format_v2_typed_encode_ok = toon_format_v2_typed.is_some();
    let toon_rs_typed_encode_ok = toon_rs_typed.is_some();
    let toon_rust_typed_encode_ok = toon_rust_typed.is_some();
    let rtoon_typed_encode_ok = rtoon_typed.is_some();

    let serde_toon_v3_typed_ok = serde_toon_v3_typed
        .as_deref()
        .map_or(false, |encoded| {
            serde_toon_v3::from_str::<Vec<GitHubRepo>>(encoded).is_ok()
        });
    let mut serde_toon_v2_typed_decode_via_value = false;
    let mut serde_toon_v2_typed_fixed: Option<String> = None;
    let mut serde_toon_v2_typed_patched = false;
    let mut serde_toon_v2_typed_ok = serde_toon_v2_typed
        .as_deref()
        .map_or(false, |encoded| {
            serde_toon_v2::from_str::<Vec<GitHubRepo>>(encoded).is_ok()
        });
    if !serde_toon_v2_typed_ok {
        let patched_ok = serde_toon_v2_typed
            .as_deref()
            .and_then(|encoded| serde_toon_v2::from_str::<serde_toon_v2::Value>(encoded).ok())
            .and_then(|value| serde_json::to_value(value).ok())
            .and_then(|value| serde_json::from_value::<Vec<GitHubRepo>>(value).ok())
            .is_some();
        if patched_ok {
            serde_toon_v2_typed_ok = true;
            serde_toon_v2_typed_decode_via_value = true;
        }
    }
    if !serde_toon_v2_typed_ok {
        if let Some(encoded) = serde_toon_v2_typed.as_deref() {
            let patched = patch_values_only(encoded, &should_quote_serde_toon_v2);
            let patched_json_ok =
                serde_toon_v2::from_str::<Vec<GitHubRepo>>(&patched).is_ok();
            let patched_value_ok = serde_toon_v2::from_str::<serde_toon_v2::Value>(&patched)
                .ok()
                .and_then(|value| serde_json::to_value(value).ok())
                .and_then(|value| serde_json::from_value::<Vec<GitHubRepo>>(value).ok())
                .is_some();
            if patched_json_ok || patched_value_ok {
                serde_toon_v2_typed_fixed = Some(patched);
                serde_toon_v2_typed_ok = true;
                serde_toon_v2_typed_patched = true;
                serde_toon_v2_typed_decode_via_value = !patched_json_ok;
            }
        }
    }
    let mut toon_format_v2_typed_ok = toon_format_v2_typed
        .as_deref()
        .map_or(false, |encoded| {
            toon_format::decode::<Vec<GitHubRepo>>(encoded, &toon_format_decode_opts_typed)
                .is_ok()
        });
    let toon_rs_typed_ok = toon_rs_typed
        .as_deref()
        .and_then(|encoded| {
            toon_rs::decode_from_str::<serde_json::Value>(encoded, &toon_rs_opts)
                .ok()
        })
        .and_then(|value| serde_json::from_value::<Vec<GitHubRepo>>(value).ok())
        .is_some();
    let mut toon_rust_typed_ok = toon_rust_typed
        .as_deref()
        .and_then(|encoded| {
            toon_rust::decode(encoded, Some(&toon_rust_decode_opts_typed)).ok()
        })
        .and_then(|value| serde_json::from_value::<Vec<GitHubRepo>>(value).ok())
        .is_some();
    let mut rtoon_typed_fixed: Option<String> = None;
    let mut rtoon_typed_delimiter_patched = false;
    let mut rtoon_typed_strict_patched = false;
    let mut rtoon_typed_token_patched = false;
    let mut rtoon_typed_ok = rtoon_typed
        .as_deref()
        .and_then(|encoded| {
            rtoon::from_toon::<Vec<GitHubRepo>>(encoded, Some(&rtoon_decode_opts_typed))
                .ok()
        })
        .is_some();
    let json_typed_ok =
        serde_json::from_str::<Vec<GitHubRepo>>(&json_typed).is_ok();

    let mut toon_format_typed_patched = false;
    if !toon_format_v2_typed_ok {
        let patched_opts = toon_format::DecodeOptions::new()
            .with_strict(false)
            .with_coerce_types(false);
        let patched_ok = toon_format_v2_typed
            .as_deref()
            .map_or(false, |encoded| {
                toon_format::decode::<Vec<GitHubRepo>>(encoded, &patched_opts).is_ok()
            });
        if patched_ok {
            toon_format_decode_opts_typed = patched_opts;
            toon_format_v2_typed_ok = true;
            toon_format_typed_patched = true;
        }
    }

    let mut toon_rust_typed_patched = false;
    if !toon_rust_typed_ok {
        let patched_opts = toon_rust::DecodeOptions::new().strict(false);
        let patched_ok = toon_rust_typed
            .as_deref()
            .and_then(|encoded| toon_rust::decode(encoded, Some(&patched_opts)).ok())
            .and_then(|value| serde_json::from_value::<Vec<GitHubRepo>>(value).ok())
            .is_some();
        if patched_ok {
            toon_rust_decode_opts_typed = patched_opts;
            toon_rust_typed_ok = true;
            toon_rust_typed_patched = true;
        }
    }

    if !rtoon_typed_ok {
        let patched_opts = rtoon::DecodeOptions::new().with_delimiter(rtoon::Delimiter::Comma);
        let patched_ok = rtoon_typed
            .as_deref()
            .and_then(|encoded| {
                rtoon::from_toon::<Vec<GitHubRepo>>(encoded, Some(&patched_opts)).ok()
            })
            .is_some();
        if patched_ok {
            rtoon_decode_opts_typed = patched_opts;
            rtoon_typed_ok = true;
            rtoon_typed_delimiter_patched = true;
        }
    }

    if !rtoon_typed_ok {
        let patched_opts = rtoon_decode_opts_typed.clone().with_strict(false);
        let patched_ok = rtoon_typed
            .as_deref()
            .and_then(|encoded| {
                rtoon::from_toon::<Vec<GitHubRepo>>(encoded, Some(&patched_opts)).ok()
            })
            .is_some();
        if patched_ok {
            rtoon_decode_opts_typed = patched_opts;
            rtoon_typed_ok = true;
            rtoon_typed_strict_patched = true;
        }
    }

    if !rtoon_typed_ok {
        if let Some(encoded) = rtoon_typed.as_deref() {
            let patched = patch_values_only(encoded, &should_quote_digit_leading);
            let patched_ok = rtoon::from_toon::<Vec<GitHubRepo>>(
                &patched,
                Some(&rtoon_decode_opts_typed),
            )
            .is_ok();
            if patched_ok {
                rtoon_typed_fixed = Some(patched);
                rtoon_typed_ok = true;
                rtoon_typed_token_patched = true;
            }
        }
    }

    let serde_toon_v2_typed_for_decode =
        serde_toon_v2_typed_fixed.as_deref().or(serde_toon_v2_typed.as_deref());
    let rtoon_typed_for_decode =
        rtoon_typed_fixed.as_deref().or(rtoon_typed.as_deref());

    report_decode_failure("typed", "serde_toon_v3", serde_toon_v3_typed_ok);
    if serde_toon_v2_typed_decode_via_value {
        report_patch("typed", "serde_toon_v2", "decode via serde_toon::Value");
    }
    if serde_toon_v2_typed_patched {
        report_patch("typed", "serde_toon_v2", "quoted t/f/n + digit-leading strings");
    }
    report_decode_failure("typed", "serde_toon_v2", serde_toon_v2_typed_ok);
    if toon_format_typed_patched {
        report_patch("typed", "toon_format_v2", "strict=false, coerce_types=false");
    }
    report_decode_failure("typed", "toon_format_v2", toon_format_v2_typed_ok);
    report_decode_failure("typed", "toon_rs_v3", toon_rs_typed_ok);
    if toon_rust_typed_encode_ok {
        report_patch("typed", "toon_rust_v1", "sanitized '|' -> \\u007C");
    }
    if toon_rust_typed_patched {
        report_patch("typed", "toon_rust_v1", "strict=false");
    }
    report_decode_failure("typed", "toon_rust_v1", toon_rust_typed_ok);
    if rtoon_typed_delimiter_patched {
        report_patch("typed", "rtoon_v1", "delimiter=comma");
    }
    if rtoon_typed_strict_patched {
        report_patch("typed", "rtoon_v1", "strict=false");
    }
    if rtoon_typed_token_patched {
        report_patch("typed", "rtoon_v1", "quoted digit-leading strings");
    }
    report_decode_failure("typed", "rtoon_v1", rtoon_typed_ok);
    report_decode_failure("typed", "serde_json", json_typed_ok);

    if !serde_toon_v2_typed_ok {
        if let Some(encoded) = serde_toon_v2_typed_for_decode {
            if let Err(err) = serde_toon_v2::from_str::<Vec<GitHubRepo>>(encoded) {
                report_decode_error("typed", "serde_toon_v2", &err.to_string());
            }
        }
    }
    if !toon_rust_typed_ok {
        if let Some(encoded) = toon_rust_typed.as_deref() {
            match toon_rust::decode(encoded, Some(&toon_rust_decode_opts_typed)) {
                Err(err) => {
                    report_decode_error("typed", "toon_rust_v1", &err.to_string());
                }
                Ok(value) => {
                    if let Err(err) = serde_json::from_value::<Vec<GitHubRepo>>(value) {
                        report_decode_error("typed", "toon_rust_v1", &err.to_string());
                    }
                }
            }
        }
    }
    if !rtoon_typed_ok {
        if let Some(encoded) = rtoon_typed_for_decode {
            match rtoon::from_toon::<Vec<GitHubRepo>>(encoded, Some(&rtoon_decode_opts_typed)) {
                Err(err) => {
                    report_decode_error("typed", "rtoon_v1", &err.to_string());
                }
                Ok(_) => {}
            }
        }
    }

    let mut group = c.benchmark_group("github_repos_typed");

    if serde_toon_v3_typed_encode_ok {
        group.bench_function("encode_serde_toon_v3", |b| {
            b.iter(|| {
                let encoded = serde_toon_v3::to_string(black_box(&repos))
                    .expect("serde_toon_v3 encode failed");
                black_box(encoded);
            });
        });
    }

    if serde_toon_v3_typed_ok {
        let encoded = serde_toon_v3_typed.as_ref().expect("encoded");
        group.bench_function("decode_serde_toon_v3", |b| {
            b.iter(|| {
                let decoded: Vec<GitHubRepo> =
                    serde_toon_v3::from_str(black_box(encoded.as_str()))
                        .expect("serde_toon_v3 decode failed");
                black_box(decoded);
            });
        });
    }

    if serde_toon_v2_typed_encode_ok {
        group.bench_function("encode_serde_toon_v2", |b| {
            b.iter(|| {
                let encoded = serde_toon_v2::to_string(black_box(&repos))
                    .expect("serde_toon_v2 encode failed");
                black_box(encoded);
            });
        });
    }

    if serde_toon_v2_typed_ok {
        let encoded = serde_toon_v2_typed_for_decode.expect("encoded");
        group.bench_function("decode_serde_toon_v2", |b| {
            b.iter(|| {
                if serde_toon_v2_typed_decode_via_value {
                    let decoded: serde_toon_v2::Value =
                        serde_toon_v2::from_str(black_box(encoded))
                            .expect("serde_toon_v2 decode failed");
                    let decoded_json =
                        serde_json::to_value(decoded)
                            .expect("serde_toon_v2 to serde_json failed");
                    let decoded_struct: Vec<GitHubRepo> =
                        serde_json::from_value(decoded_json)
                            .expect("serde_toon_v2 decode to struct failed");
                    black_box(decoded_struct);
                } else {
                    let decoded: Vec<GitHubRepo> =
                        serde_toon_v2::from_str(black_box(encoded))
                            .expect("serde_toon_v2 decode failed");
                    black_box(decoded);
                }
            });
        });
    }

    if toon_format_v2_typed_encode_ok {
        group.bench_function("encode_toon_format_v2", |b| {
            b.iter(|| {
                let encoded = toon_format::encode_default(black_box(&repos))
                    .expect("toon-format encode failed");
                black_box(encoded);
            });
        });
    }

    if toon_format_v2_typed_ok {
        let encoded = toon_format_v2_typed.as_ref().expect("encoded");
        group.bench_function("decode_toon_format_v2", |b| {
            b.iter(|| {
                let decoded: Vec<GitHubRepo> =
                    toon_format::decode(
                        black_box(encoded.as_str()),
                        &toon_format_decode_opts_typed,
                    )
                        .expect("toon-format decode failed");
                black_box(decoded);
            });
        });
    }

    if toon_rs_typed_encode_ok {
        group.bench_function("encode_toon_rs_v3", |b| {
            b.iter(|| {
                let encoded = toon_rs::encode_to_string(black_box(&repos), &toon_rs_opts)
                    .expect("toon-rs encode failed");
                black_box(encoded);
            });
        });
    }

    if toon_rs_typed_ok {
        let encoded = toon_rs_typed.as_ref().expect("encoded");
        group.bench_function("decode_toon_rs_v3", |b| {
            b.iter(|| {
                let decoded_value: serde_json::Value =
                    toon_rs::decode_from_str(black_box(encoded.as_str()), &toon_rs_opts)
                        .expect("toon-rs decode failed");
                let decoded: Vec<GitHubRepo> =
                    serde_json::from_value(decoded_value)
                        .expect("toon-rs decode to struct failed");
                black_box(decoded);
            });
        });
    }

    if toon_rust_typed_encode_ok {
        group.bench_function("encode_toon_rust_v1", |b| {
            b.iter(|| {
                let encoded = toon_rust::serde_api::to_string_with_options(
                    black_box(&toon_rust_repos),
                    &toon_rust_encode_opts,
                )
                .expect("toon-rust encode failed");
                black_box(encoded);
            });
        });
    }

    if toon_rust_typed_ok {
        let encoded = toon_rust_typed.as_ref().expect("encoded");
        group.bench_function("decode_toon_rust_v1", |b| {
            b.iter(|| {
                let decoded_value = toon_rust::decode(
                    black_box(encoded.as_str()),
                    Some(&toon_rust_decode_opts_typed),
                )
                .expect("toon-rust decode failed");
                let decoded: Vec<GitHubRepo> =
                    serde_json::from_value(decoded_value)
                        .expect("toon-rust decode to struct failed");
                black_box(decoded);
            });
        });
    }

    if rtoon_typed_encode_ok {
        group.bench_function("encode_rtoon_v1", |b| {
            b.iter(|| {
                let encoded = rtoon::to_toon(black_box(&repos), Some(&rtoon_encode_opts))
                    .expect("rtoon encode failed");
                black_box(encoded);
            });
        });
    }

    if rtoon_typed_ok {
        let encoded = rtoon_typed_for_decode.expect("encoded");
        group.bench_function("decode_rtoon_v1", |b| {
            b.iter(|| {
                let decoded: Vec<GitHubRepo> = rtoon::from_toon(
                    black_box(encoded),
                    Some(&rtoon_decode_opts_typed),
                )
                .expect("rtoon decode failed");
                black_box(decoded);
            });
        });
    }

    group.bench_function("encode_json", |b| {
        b.iter(|| {
            let encoded = serde_json::to_string(black_box(&repos))
                .expect("json encode failed");
            black_box(encoded);
        });
    });

    group.bench_function("decode_json", |b| {
        b.iter(|| {
            let decoded: Vec<GitHubRepo> =
                serde_json::from_str(black_box(json_typed.as_str()))
                    .expect("json decode failed");
            black_box(decoded);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_github_repos);
criterion_main!(benches);
