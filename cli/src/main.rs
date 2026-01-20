use std::borrow::Cow;
use std::error::Error;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use clap::{ArgAction, Parser, ValueEnum};
use serde::Serialize;
use serde_json::Value;
use serde_toon::{DecodeOptions, Delimiter, EncodeOptions, ExpandPaths, Indent, KeyFolding};
use tiktoken_rs::cl100k_base;

#[derive(Parser, Debug)]
#[command(name = "toon", version, about = "TOON encoder/decoder")]
struct Args {
    /// Input file path (.json or .toon). Omit or use '-' to read from stdin.
    input: Option<String>,

    /// Output file path (prints to stdout if omitted).
    #[arg(short, long, value_name = "file")]
    output: Option<String>,

    /// Force encode mode (overrides auto-detection).
    #[arg(short = 'e', long)]
    encode: bool,

    /// Force decode mode (overrides auto-detection).
    #[arg(short = 'd', long)]
    decode: bool,

    /// Array delimiter: , (comma), \\t (tab), | (pipe).
    #[arg(long, value_name = "char", value_parser = parse_delimiter)]
    delimiter: Option<Delimiter>,

    /// Indentation size (default: 2).
    #[arg(long, value_name = "number", default_value_t = 2)]
    indent: usize,

    /// Show token statistics.
    #[arg(long)]
    stats: bool,

    /// Key folding mode: off, safe (default: off).
    #[arg(long = "keyFolding", alias = "key-folding", value_enum, value_name = "mode", default_value_t = KeyFoldingArg::Off)]
    key_folding: KeyFoldingArg,

    /// Maximum folded segment count when key folding is enabled (default: Infinity).
    #[arg(long = "flattenDepth", alias = "flatten-depth", value_name = "number")]
    flatten_depth: Option<usize>,

    /// Path expansion mode: off, safe (default: off).
    #[arg(long = "expandPaths", alias = "expand-paths", value_enum, value_name = "mode", default_value_t = ExpandPathsArg::Off)]
    expand_paths: ExpandPathsArg,

    /// Disable strict validation when decoding.
    #[arg(long = "no-strict", action = ArgAction::SetFalse, default_value_t = true)]
    strict: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum KeyFoldingArg {
    Off,
    Safe,
}

impl From<KeyFoldingArg> for KeyFolding {
    fn from(value: KeyFoldingArg) -> Self {
        match value {
            KeyFoldingArg::Off => KeyFolding::Off,
            KeyFoldingArg::Safe => KeyFolding::Safe,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum ExpandPathsArg {
    Off,
    Safe,
}

impl From<ExpandPathsArg> for ExpandPaths {
    fn from(value: ExpandPathsArg) -> Self {
        match value {
            ExpandPathsArg::Off => ExpandPaths::Off,
            ExpandPathsArg::Safe => ExpandPaths::Safe,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Encode,
    Decode,
}

#[derive(Debug)]
enum InputSource {
    Stdin,
    File(String),
}

fn main() {
    if let Err(err) = run() {
        eprintln!("ERROR  {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let (input_text, input_source) = read_input(args.input.as_deref())?;
    let mode = resolve_mode(&args, &input_source)?;

    match mode {
        Mode::Encode => run_encode(&args, &input_text, &input_source),
        Mode::Decode => run_decode(&args, &input_text, &input_source),
    }
}

fn run_encode(args: &Args, input: &str, input_source: &InputSource) -> Result<(), Box<dyn Error>> {
    let value: Value = serde_json::from_str(input)?;
    let mut options = EncodeOptions::new().with_indent(Indent::Spaces(args.indent));

    if let Some(delimiter) = args.delimiter {
        options = options.with_delimiter(delimiter);
    }

    options = options.with_key_folding(args.key_folding.into());

    if let Some(flatten_depth) = args.flatten_depth {
        options = options.with_flatten_depth(Some(flatten_depth));
    }

    let output_target = OutputTarget::from_arg(args.output.as_deref());

    if args.stats {
        let toon = serde_toon::to_string_with_options(&value, &options)?;
        write_output(output_target.path(), toon.as_bytes())?;
        let leading_newlines = if let OutputTarget::File(path) = &output_target {
            report_status(Mode::Encode, input_source, path);
            1
        } else {
            2
        };
        print_stats(&value, &toon, leading_newlines)?;
        return Ok(());
    }

    with_output_writer(output_target.path(), |writer| {
        serde_toon::to_writer_with_options(writer, &value, &options).map_err(|err| err.into())
    })?;
    if let OutputTarget::File(path) = &output_target {
        report_status(Mode::Encode, input_source, path);
    }
    Ok(())
}

fn run_decode(args: &Args, input: &str, input_source: &InputSource) -> Result<(), Box<dyn Error>> {
    let options = DecodeOptions::new()
        .with_indent(Indent::Spaces(args.indent))
        .with_strict(args.strict)
        .with_expand_paths(args.expand_paths.into());

    let normalized = if args.strict || !input.contains('\t') {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(normalize_non_strict_tabs(input))
    };

    let value: Value = serde_toon::from_str_with_options(&normalized, &options)?;
    let output_target = OutputTarget::from_arg(args.output.as_deref());

    with_output_writer(output_target.path(), |writer| {
        write_json(writer, &value, args.indent)
    })?;
    if let OutputTarget::File(path) = &output_target {
        report_status(Mode::Decode, input_source, path);
    }
    Ok(())
}

fn resolve_mode(args: &Args, input_source: &InputSource) -> Result<Mode, Box<dyn Error>> {
    if args.encode {
        return Ok(Mode::Encode);
    }

    if args.decode {
        return Ok(Mode::Decode);
    }

    match input_source {
        InputSource::Stdin => Ok(Mode::Encode),
        InputSource::File(path) => match Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("json") => Ok(Mode::Encode),
            Some("toon") => Ok(Mode::Decode),
            _ => Err("unable to auto-detect mode; use --encode or --decode".into()),
        },
    }
}

fn read_input(input: Option<&str>) -> Result<(String, InputSource), Box<dyn Error>> {
    match input {
        None | Some("-") => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok((buf, InputSource::Stdin))
        }
        Some(path) => {
            let buf = fs::read_to_string(path)?;
            Ok((buf, InputSource::File(path.to_string())))
        }
    }
}

fn parse_delimiter(raw: &str) -> Result<Delimiter, String> {
    match raw {
        "," => Ok(Delimiter::Comma),
        "|" => Ok(Delimiter::Pipe),
        "\t" => Ok(Delimiter::Tab),
        _ => Err(format!(
            "Invalid delimiter \"{raw}\". Valid delimiters are: comma (,), tab (\\t), pipe (|)"
        )),
    }
}

#[derive(Clone, Debug)]
enum OutputTarget {
    Stdout,
    File(String),
}

impl OutputTarget {
    fn from_arg(output: Option<&str>) -> Self {
        match output {
            Some(path) if path != "-" => OutputTarget::File(path.to_string()),
            _ => OutputTarget::Stdout,
        }
    }

    fn path(&self) -> Option<&str> {
        match self {
            OutputTarget::Stdout => None,
            OutputTarget::File(path) => Some(path.as_str()),
        }
    }
}

fn with_output_writer<F>(path: Option<&str>, f: F) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(&mut dyn Write) -> Result<(), Box<dyn Error>>,
{
    match path {
        Some(path) if path != "-" => {
            let mut file = fs::File::create(path)?;
            f(&mut file)
        }
        _ => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            f(&mut handle)
        }
    }
}

fn write_output(path: Option<&str>, data: &[u8]) -> Result<(), Box<dyn Error>> {
    with_output_writer(path, |writer| {
        writer.write_all(data)?;
        Ok(())
    })
}

fn write_json(writer: &mut dyn Write, value: &Value, indent: usize) -> Result<(), Box<dyn Error>> {
    if indent == 0 {
        serde_json::to_writer(writer, value)?;
        return Ok(());
    }

    let indent_bytes = vec![b' '; indent];
    let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent_bytes);
    let mut serializer = serde_json::Serializer::with_formatter(writer, formatter);
    value.serialize(&mut serializer)?;
    Ok(())
}

fn report_status(mode: Mode, input_source: &InputSource, output_path: &str) {
    let input_label = match input_source {
        InputSource::Stdin => "stdin".to_string(),
        InputSource::File(path) => display_path(path),
    };
    let output_label = display_path(output_path);
    let verb = match mode {
        Mode::Encode => "Encoded",
        Mode::Decode => "Decoded",
    };
    println!("✔ {verb} {input_label} → {output_label}");
}

fn print_stats(value: &Value, toon: &str, leading_newlines: usize) -> Result<(), Box<dyn Error>> {
    let json = serde_json::to_string(value)?;
    let bpe = cl100k_base()?;
    let json_tokens = count_tokens(&bpe, &json);
    let toon_tokens = count_tokens(&bpe, toon);
    let saved = json_tokens as isize - toon_tokens as isize;
    let pct = if json_tokens > 0 {
        ((toon_tokens as f64 - json_tokens as f64) / json_tokens as f64) * 100.0
    } else {
        0.0
    };

    for _ in 0..leading_newlines {
        println!();
    }
    println!("ℹ Token estimates: ~{json_tokens} (JSON) → ~{toon_tokens} (TOON)");
    println!("✔ Saved ~{saved} tokens ({pct:.1}%)");
    Ok(())
}

fn count_tokens(bpe: &tiktoken_rs::CoreBPE, text: &str) -> usize {
    bpe.encode_with_special_tokens(text).len()
}

fn display_path(path: &str) -> String {
    let path = Path::new(path);
    let Ok(cwd) = std::env::current_dir() else {
        return path.to_string_lossy().into_owned();
    };
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let rel = diff_paths(&abs, &cwd).unwrap_or(abs);
    rel.to_string_lossy().into_owned()
}

fn diff_paths(path: &Path, base: &Path) -> Option<std::path::PathBuf> {
    let path_components: Vec<_> = path.components().collect();
    let base_components: Vec<_> = base.components().collect();

    if path_components.first()? != base_components.first()? {
        return None;
    }

    let mut common = 0;
    while common < path_components.len()
        && common < base_components.len()
        && path_components[common] == base_components[common]
    {
        common += 1;
    }

    let mut result = std::path::PathBuf::new();
    for _ in common..base_components.len() {
        result.push("..");
    }
    for component in &path_components[common..] {
        result.push(component.as_os_str());
    }

    Some(result)
}

// Match the JS CLI: in non-strict mode, lines with tab-indentation lose indentation entirely.
fn normalize_non_strict_tabs(input: &str) -> String {
    let mut out = String::with_capacity(input.len());

    for line in input.split_inclusive('\n') {
        let (content, newline) = match line.strip_suffix('\n') {
            Some(stripped) => (stripped, "\n"),
            None => (line, ""),
        };

        let mut saw_tab = false;
        let mut first_non_ws = None;
        for (idx, ch) in content.char_indices() {
            match ch {
                '\t' => saw_tab = true,
                ' ' => {}
                _ => {
                    first_non_ws = Some(idx);
                    break;
                }
            }
        }

        let trimmed = if saw_tab {
            let start = first_non_ws.unwrap_or(content.len());
            &content[start..]
        } else {
            content
        };

        out.push_str(trimmed);
        out.push_str(newline);
    }

    out
}
