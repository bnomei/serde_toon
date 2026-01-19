use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use csv::Writer;
use regex::Regex;
use roxmltree::Document;
use serde::Serialize;

#[derive(Debug)]
struct Node {
    function: String,
    samples: u64,
    percent: f64,
    x: String,
    y: String,
    width: String,
    height: String,
}

#[derive(Serialize)]
struct TopItem {
    function: String,
    samples: u64,
    percent: f64,
}

struct Args {
    svg: PathBuf,
    out_prefix: Option<PathBuf>,
}

fn parse_args() -> Result<Args, Box<dyn Error>> {
    let mut svg: Option<PathBuf> = None;
    let mut out_prefix: Option<PathBuf> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-prefix" => {
                let value = args.next().ok_or("--out-prefix requires a value")?;
                out_prefix = Some(PathBuf::from(value));
            }
            "--help" | "-h" => {
                return Err("usage: flamegraph_to_csv <svg> [--out-prefix PATH]".into());
            }
            _ if svg.is_none() => {
                svg = Some(PathBuf::from(arg));
            }
            _ => {
                return Err(format!("unknown arg: {arg}").into());
            }
        }
    }

    let svg = svg.ok_or("usage: flamegraph_to_csv <svg> [--out-prefix PATH]")?;
    Ok(Args { svg, out_prefix })
}

fn parse_svg(svg_path: &Path) -> Result<Vec<Node>, Box<dyn Error>> {
    let svg_text = fs::read_to_string(svg_path)?;
    let doc = Document::parse(&svg_text)?;
    let title_re = Regex::new(r"^(.*) \((\d+) samples?, ([0-9.]+)%\)$")?;

    let mut nodes = Vec::new();

    for g in doc
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "g")
    {
        let mut title_text: Option<&str> = None;
        let mut rect_node = None;

        for child in g.children().filter(|node| node.is_element()) {
            match child.tag_name().name() {
                "title" => title_text = child.text(),
                "rect" => rect_node = Some(child),
                _ => {}
            }
            if title_text.is_some() && rect_node.is_some() {
                break;
            }
        }

        let (Some(title_text), Some(rect)) = (title_text, rect_node) else {
            continue;
        };
        let title = title_text.trim();
        let captures = match title_re.captures(title) {
            Some(captures) => captures,
            None => continue,
        };
        let function = captures
            .get(1)
            .ok_or("missing function name in title")?
            .as_str()
            .to_string();
        let samples: u64 = captures
            .get(2)
            .ok_or("missing samples in title")?
            .as_str()
            .parse()?;
        let percent: f64 = captures
            .get(3)
            .ok_or("missing percent in title")?
            .as_str()
            .parse()?;

        let attr = |name| rect.attribute(name).unwrap_or("").to_string();
        nodes.push(Node {
            function,
            samples,
            percent,
            x: attr("x"),
            y: attr("y"),
            width: attr("width"),
            height: attr("height"),
        });
    }

    Ok(nodes)
}

fn write_nodes_csv(nodes: &[Node], out_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut writer = Writer::from_path(out_path)?;
    writer.write_record([
        "function", "samples", "percent", "x", "y", "width", "height",
    ])?;
    for node in nodes {
        writer.write_record([
            node.function.as_str(),
            &node.samples.to_string(),
            &format!("{:.2}", node.percent),
            node.x.as_str(),
            node.y.as_str(),
            node.width.as_str(),
            node.height.as_str(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

fn write_top_json(nodes: &[Node], out_path: &Path) -> Result<(), Box<dyn Error>> {
    let mut counts: HashMap<String, u64> = HashMap::new();
    for node in nodes {
        *counts.entry(node.function.clone()).or_default() += node.samples;
    }
    let total = counts
        .get("all")
        .copied()
        .unwrap_or_else(|| counts.values().sum());

    let mut items: Vec<(String, u64)> = counts.into_iter().collect();
    items.sort_by(|(name_a, samples_a), (name_b, samples_b)| {
        samples_b.cmp(samples_a).then_with(|| name_a.cmp(name_b))
    });

    let items: Vec<TopItem> = items
        .into_iter()
        .map(|(function, samples)| TopItem {
            function,
            samples,
            percent: if total == 0 {
                0.0
            } else {
                samples as f64 / total as f64 * 100.0
            },
        })
        .collect();

    let file = fs::File::create(out_path)?;
    serde_json::to_writer_pretty(file, &items)?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    if !args.svg.exists() {
        return Err(format!("SVG not found: {}", args.svg.display()).into());
    }

    let prefix = args
        .out_prefix
        .unwrap_or_else(|| args.svg.with_extension(""));
    let nodes_csv = prefix.with_extension("nodes.csv");
    let top_json = prefix.with_extension("top.json");

    let nodes = parse_svg(&args.svg)?;
    write_nodes_csv(&nodes, &nodes_csv)?;
    write_top_json(&nodes, &top_json)?;

    println!(
        "Wrote {} and {} (nodes={})",
        nodes_csv.display(),
        top_json.display(),
        nodes.len()
    );
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
