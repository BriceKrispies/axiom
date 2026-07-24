//! `axiom-font-import` — the offline font compiler CLI.
//!
//! Converts a TTF/OTF/WOFF font into a deterministic `.axfont` runtime asset.
//! WOFF2 must be pre-decompressed. Usage:
//!
//! ```text
//! axiom-font-import import --input assets/source/MyFont.woff2 \
//!   --output assets/fonts/my-font.axfont \
//!   --sizes 32 --ranges U+0020-007E,U+00A0-00FF
//! ```

mod import;
mod ranges;
mod woff;

use std::path::PathBuf;
use std::process::ExitCode;

use import::ImportOptions;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(summary) => {
            eprintln!("{summary}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("axiom-font-import: error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Parsed CLI options for the `import` subcommand.
struct Cli {
    input: PathBuf,
    output: PathBuf,
    sizes: Vec<u32>,
    ranges: String,
    family: Option<String>,
    atlas_width: u32,
    atlas_height: u32,
    padding: u32,
    replacement: u32,
    overwrite: bool,
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("import") => run_import(&args[1..]),
        Some(other) => Err(format!("unknown subcommand '{other}' (expected 'import')")),
        None => Err(
            "usage: axiom-font-import import --input <f> --output <f> --sizes <n> --ranges <spec>"
                .to_owned(),
        ),
    }
}

fn run_import(args: &[String]) -> Result<String, String> {
    let cli = parse_cli(args)?;
    let pixel_size = *cli.sizes.first().ok_or("no --sizes given")?;
    if cli.sizes.len() > 1 {
        eprintln!(
            "note: this build emits one size layer; using --sizes {pixel_size} (multi-size is a documented future extension)"
        );
    }
    let codepoints = ranges::parse_ranges(&cli.ranges)?;
    if codepoints.is_empty() {
        return Err("--ranges resolved to no codepoints".to_owned());
    }
    if cli.output.exists() && !cli.overwrite {
        return Err(format!(
            "output {} exists (pass --overwrite)",
            cli.output.display()
        ));
    }

    let source =
        std::fs::read(&cli.input).map_err(|e| format!("read {}: {e}", cli.input.display()))?;
    let sfnt = woff::to_sfnt(&source)?;
    let family = cli
        .family
        .clone()
        .or_else(|| {
            cli.input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Imported".to_owned());

    let options = ImportOptions {
        pixel_size,
        codepoints: codepoints.clone(),
        family: family.clone(),
        atlas_width: cli.atlas_width,
        atlas_height: cli.atlas_height,
        padding: cli.padding,
        replacement: cli.replacement,
    };
    let bytes = import::compile(&sfnt, &options)?;

    write_atomic(&cli.output, &bytes)?;
    Ok(format!(
        "wrote {} ({} bytes, family '{}', {} codepoints requested at {}px)",
        cli.output.display(),
        bytes.len(),
        family,
        codepoints.len(),
        pixel_size
    ))
}

/// Write `bytes` to `path` atomically (temp file + rename).
fn write_atomic(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("axfont.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))
}

fn parse_cli(args: &[String]) -> Result<Cli, String> {
    let mut input = None;
    let mut output = None;
    let mut sizes = Vec::new();
    let mut ranges = "U+0020-007E".to_owned();
    let mut family = None;
    let mut atlas_width = 512u32;
    let mut atlas_height = 512u32;
    let mut padding = 1u32;
    let mut replacement = 0xFFFDu32;
    let mut overwrite = false;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| format!("missing value for {flag}"))
        };
        match flag {
            "--input" => input = Some(PathBuf::from(value(&mut i)?)),
            "--output" => output = Some(PathBuf::from(value(&mut i)?)),
            "--sizes" => sizes = parse_sizes(&value(&mut i)?)?,
            "--ranges" => ranges = value(&mut i)?,
            "--family" => family = Some(value(&mut i)?),
            "--atlas-width" => atlas_width = parse_u32(&value(&mut i)?, "--atlas-width")?,
            "--atlas-height" => atlas_height = parse_u32(&value(&mut i)?, "--atlas-height")?,
            "--padding" => padding = parse_u32(&value(&mut i)?, "--padding")?,
            "--replacement" => {
                replacement = ranges::parse_ranges(&value(&mut i)?)?
                    .first()
                    .copied()
                    .unwrap_or(0xFFFD)
            }
            "--overwrite" => overwrite = true,
            other => return Err(format!("unknown flag '{other}'")),
        }
        i += 1;
    }
    Ok(Cli {
        input: input.ok_or("missing --input")?,
        output: output.ok_or("missing --output")?,
        sizes,
        ranges,
        family,
        atlas_width,
        atlas_height,
        padding,
        replacement,
        overwrite,
    })
}

fn parse_sizes(spec: &str) -> Result<Vec<u32>, String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| parse_u32(s, "--sizes"))
        .collect()
}

fn parse_u32(token: &str, flag: &str) -> Result<u32, String> {
    token
        .parse::<u32>()
        .map_err(|_| format!("{flag}: expected a number, got '{token}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_command_line() {
        let args: Vec<String> =
            "--input a.ttf --output b.axfont --sizes 16,32 --ranges U+0041-0042 --overwrite"
                .split(' ')
                .map(String::from)
                .collect();
        let cli = parse_cli(&args).unwrap();
        assert_eq!(cli.sizes, vec![16, 32]);
        assert_eq!(cli.ranges, "U+0041-0042");
        assert!(cli.overwrite);
        assert_eq!(cli.atlas_width, 512);
    }

    #[test]
    fn rejects_missing_required_and_unknown_flags() {
        assert!(parse_cli(&["--sizes".into(), "16".into()]).is_err());
        assert!(parse_cli(&["--bogus".into()]).is_err());
        assert!(run(&["frobnicate".into()]).is_err());
        assert!(run(&[]).is_err());
    }

    #[test]
    fn parses_sizes_and_numbers() {
        assert_eq!(parse_sizes("16, 24 ,32").unwrap(), vec![16, 24, 32]);
        assert!(parse_u32("x", "--padding").is_err());
    }
}
