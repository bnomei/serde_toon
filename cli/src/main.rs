use std::error::Error;
use std::fs;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
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
    #[arg(long, value_name = "number", default_value_t = 2, value_parser = parse_indent)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModeSelection {
    Fixed(Mode),
    AutoDetect,
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
    let input_source = input_source_from_arg(args.input.as_deref());
    let mode = resolve_mode(&args, &input_source)?;

    match mode {
        ModeSelection::Fixed(Mode::Encode) => {
            let input_text = read_input_text(&input_source)?;
            run_encode(&args, &input_text, &input_source)
        }
        ModeSelection::Fixed(Mode::Decode) => run_decode(&args, &input_source),
        ModeSelection::AutoDetect => run_auto_detect(&args, &input_source),
    }
}

fn run_encode(args: &Args, input: &str, input_source: &InputSource) -> Result<(), Box<dyn Error>> {
    let value: Value = serde_json::from_str(input)?;
    run_encode_value(args, &value, input_source)
}

fn run_encode_value(
    args: &Args,
    value: &Value,
    input_source: &InputSource,
) -> Result<(), Box<dyn Error>> {
    let options = build_encode_options(args);
    let output_target = OutputTarget::from_arg(args.output.as_deref());

    if args.stats {
        let toon = serde_toon::to_string_with_options(value, &options)?;
        write_output(output_target.path(), toon.as_bytes())?;
        let leading_newlines = if let OutputTarget::File(path) = &output_target {
            report_status(Mode::Encode, input_source, path);
            1
        } else {
            2
        };
        print_stats(value, &toon, leading_newlines)?;
        return Ok(());
    }

    with_output_writer(output_target.path(), |writer| {
        serde_toon::to_writer_with_options(writer, value, &options).map_err(|err| err.into())
    })?;
    if let OutputTarget::File(path) = &output_target {
        report_status(Mode::Encode, input_source, path);
    }
    Ok(())
}

fn run_decode(args: &Args, input_source: &InputSource) -> Result<(), Box<dyn Error>> {
    let reader = open_input_reader(input_source)?;
    let value = decode_value_from_reader(args, reader)?;
    run_decode_value(args, &value, input_source)
}

fn run_decode_value(
    args: &Args,
    value: &Value,
    input_source: &InputSource,
) -> Result<(), Box<dyn Error>> {
    let output_target = OutputTarget::from_arg(args.output.as_deref());

    with_output_writer(output_target.path(), |writer| {
        write_json(writer, value, args.indent)
    })?;
    if let OutputTarget::File(path) = &output_target {
        report_status(Mode::Decode, input_source, path);
    }
    Ok(())
}

fn run_auto_detect(args: &Args, input_source: &InputSource) -> Result<(), Box<dyn Error>> {
    let input = read_input_text(input_source)?;
    match detect_auto_kind(&input) {
        AutoDetectKind::Json => match serde_json::from_str::<Value>(&input) {
            Ok(value) => run_encode_value(args, &value, input_source),
            Err(json_err) => match decode_value_from_str(args, &input) {
                Ok(value) => run_decode_value(args, &value, input_source),
                Err(toon_err) => Err(auto_detect_error(json_err, toon_err)),
            },
        },
        AutoDetectKind::Toon => match decode_value_from_str(args, &input) {
            Ok(value) => run_decode_value(args, &value, input_source),
            Err(toon_err) => match serde_json::from_str::<Value>(&input) {
                Ok(value) => run_encode_value(args, &value, input_source),
                Err(json_err) => Err(auto_detect_error(json_err, toon_err)),
            },
        },
        AutoDetectKind::Uncertain => {
            Err("unable to auto-detect mode; use --encode or --decode".into())
        }
    }
}

fn resolve_mode(args: &Args, input_source: &InputSource) -> Result<ModeSelection, Box<dyn Error>> {
    if args.encode {
        return Ok(ModeSelection::Fixed(Mode::Encode));
    }

    if args.decode {
        return Ok(ModeSelection::Fixed(Mode::Decode));
    }

    match input_source {
        InputSource::Stdin => Ok(ModeSelection::Fixed(Mode::Encode)),
        InputSource::File(path) => match Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("json") => Ok(ModeSelection::Fixed(Mode::Encode)),
            Some("toon") => Ok(ModeSelection::Fixed(Mode::Decode)),
            _ => Ok(ModeSelection::AutoDetect),
        },
    }
}

fn build_encode_options(args: &Args) -> EncodeOptions {
    let mut options = EncodeOptions::new().with_indent(Indent::Spaces(args.indent));

    if let Some(delimiter) = args.delimiter {
        options = options.with_delimiter(delimiter);
    }

    options = options.with_key_folding(args.key_folding.into());

    if let Some(flatten_depth) = args.flatten_depth {
        options = options.with_flatten_depth(Some(flatten_depth));
    }

    options
}

fn build_decode_options(args: &Args) -> DecodeOptions {
    DecodeOptions::new()
        .with_indent(Indent::Spaces(args.indent))
        .with_strict(args.strict)
        .with_expand_paths(args.expand_paths.into())
}

fn decode_value_from_reader(
    args: &Args,
    reader: Box<dyn BufRead>,
) -> Result<Value, Box<dyn Error>> {
    let options = build_decode_options(args);
    if args.strict {
        Ok(serde_toon::from_reader_streaming_with_options(
            reader, &options,
        )?)
    } else {
        let normalizer = TabNormalizingReader::new(reader);
        let reader = BufReader::new(normalizer);
        Ok(serde_toon::from_reader_streaming_with_options(
            reader, &options,
        )?)
    }
}

fn decode_value_from_str(args: &Args, input: &str) -> Result<Value, Box<dyn Error>> {
    let cursor = Cursor::new(input.as_bytes());
    let reader = Box::new(BufReader::new(cursor));
    decode_value_from_reader(args, reader)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoDetectKind {
    Json,
    Toon,
    Uncertain,
}

fn detect_auto_kind(input: &str) -> AutoDetectKind {
    let first_non_ws = input.chars().find(|ch| !ch.is_whitespace());
    let toon_key_pos = find_toon_key_token(input);
    if let Some(ch) = first_non_ws {
        if ch == '{' || ch == '[' {
            if let Some(json_key_pos) = find_json_quoted_key(input) {
                if toon_key_pos.is_none() || Some(json_key_pos) < toon_key_pos {
                    return AutoDetectKind::Json;
                }
            }
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            return AutoDetectKind::Toon;
        }
    } else {
        return AutoDetectKind::Uncertain;
    }
    if toon_key_pos.is_some() {
        return AutoDetectKind::Toon;
    }
    AutoDetectKind::Uncertain
}

fn find_json_quoted_key(input: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'"' {
            let start = idx;
            idx += 1;
            let mut escape = false;
            while idx < bytes.len() {
                let byte = bytes[idx];
                if escape {
                    escape = false;
                    idx += 1;
                    continue;
                }
                if byte == b'\\' {
                    escape = true;
                    idx += 1;
                    continue;
                }
                if byte == b'"' {
                    idx += 1;
                    break;
                }
                idx += 1;
            }
            if idx >= bytes.len() {
                break;
            }
            let mut lookahead = idx;
            while lookahead < bytes.len()
                && matches!(bytes[lookahead], b' ' | b'\t' | b'\r' | b'\n')
            {
                lookahead += 1;
            }
            if lookahead < bytes.len() && bytes[lookahead] == b':' {
                return Some(start);
            }
            idx = lookahead;
            continue;
        }
        idx += 1;
    }
    None
}

fn find_toon_key_token(input: &str) -> Option<usize> {
    let mut offset = 0;
    for line in input.split_terminator('\n') {
        let bytes = line.as_bytes();
        let mut idx = 0;
        while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t' | b'\r') {
            idx += 1;
        }
        if idx < bytes.len() && is_ident_start_byte(bytes[idx]) {
            let start = offset + idx;
            idx += 1;
            while idx < bytes.len() && is_ident_continue_byte(bytes[idx]) {
                idx += 1;
            }
            while idx < bytes.len() && matches!(bytes[idx], b' ' | b'\t') {
                idx += 1;
            }
            if idx < bytes.len() && bytes[idx] == b':' {
                return Some(start);
            }
        }
        offset += line.len() + 1;
    }
    None
}

fn is_ident_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.'
}

fn auto_detect_error(json_err: serde_json::Error, toon_err: Box<dyn Error>) -> Box<dyn Error> {
    let json_err = json_err.to_string();
    let toon_err = toon_err.to_string();
    format!("input is neither valid JSON nor TOON: json error: {json_err}; toon error: {toon_err}")
        .into()
}

fn input_source_from_arg(input: Option<&str>) -> InputSource {
    match input {
        None | Some("-") => InputSource::Stdin,
        Some(path) => InputSource::File(path.to_string()),
    }
}

fn read_input_text(input_source: &InputSource) -> Result<String, Box<dyn Error>> {
    match input_source {
        InputSource::Stdin => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
        InputSource::File(path) => Ok(fs::read_to_string(path)?),
    }
}

fn open_input_reader(input_source: &InputSource) -> Result<Box<dyn BufRead>, Box<dyn Error>> {
    match input_source {
        InputSource::Stdin => Ok(Box::new(BufReader::new(io::stdin().lock()))),
        InputSource::File(path) => Ok(Box::new(BufReader::new(fs::File::open(path)?))),
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

fn parse_indent(raw: &str) -> Result<usize, String> {
    let value: usize = raw
        .parse()
        .map_err(|_| format!("Invalid indent \"{raw}\": expected a positive integer"))?;
    if value == 0 {
        return Err("indent size must be greater than zero".to_string());
    }
    Ok(value)
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
struct TabNormalizingReader<R: BufRead> {
    inner: R,
    line_buf: String,
    out_buf: Vec<u8>,
    out_pos: usize,
}

impl<R: BufRead> TabNormalizingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            line_buf: String::new(),
            out_buf: Vec::new(),
            out_pos: 0,
        }
    }

    fn refill(&mut self) -> io::Result<bool> {
        self.out_buf.clear();
        self.out_pos = 0;
        self.line_buf.clear();
        let read = self.inner.read_line(&mut self.line_buf)?;
        if read == 0 {
            return Ok(false);
        }
        let line = self.line_buf.as_str();
        let (content, newline) = if let Some(stripped) = line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = line.strip_suffix('\n') {
            let stripped = stripped.strip_suffix('\r').unwrap_or(stripped);
            (stripped, "\n")
        } else {
            let stripped = line.strip_suffix('\r').unwrap_or(line);
            (stripped, "")
        };

        let mut saw_tab = false;
        let mut first_non_ws = None;
        for (idx, byte) in content.as_bytes().iter().enumerate() {
            match byte {
                b'\t' => saw_tab = true,
                b' ' => {}
                _ => {
                    first_non_ws = Some(idx);
                    break;
                }
            }
        }

        if saw_tab {
            let start = first_non_ws.unwrap_or(content.len());
            self.out_buf.extend_from_slice(&content.as_bytes()[start..]);
        } else {
            self.out_buf.extend_from_slice(content.as_bytes());
        }
        self.out_buf.extend_from_slice(newline.as_bytes());
        Ok(true)
    }
}

impl<R: BufRead> Read for TabNormalizingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.out_pos >= self.out_buf.len() {
            if !self.refill()? {
                return Ok(0);
            }
        }
        let remaining = self.out_buf.len().saturating_sub(self.out_pos);
        let to_copy = remaining.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.out_buf[self.out_pos..self.out_pos + to_copy]);
        self.out_pos += to_copy;
        Ok(to_copy)
    }
}
