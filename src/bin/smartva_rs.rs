//! Drop-in CLI binary (`smartva-rs`) compatible with upstream `smartva` (`va_cli.py`),
//! extended with `--csv`, `--charts`, `--json`, and `--format <dir|json|json-extended|protobuf|protobuf-extended>`.
//!
//! ```text
//! smartva-rs [OPTIONS] [INPUT_CSV] [OUTPUT_DIR]
//!   --country <CODE>          Country ISO-3 code (default: "Unknown")
//!   --hiv <BOOL>              Death from HIV possible (default: True)
//!   --malaria <BOOL>          Death from Malaria possible (default: True)
//!   --hce <BOOL>              Use Health Care Experience variables (default: True)
//!   --freetext <BOOL>         Use "open response" variables (default: True)
//!   --csv <BOOL>              Write 4-folder CSV + .xlsx directory tree (default: True if OUTPUT_DIR given)
//!   --no-csv                  Skip writing CSV + .xlsx directory tree
//!   --charts <BOOL>           Generate .png charts/graphs (alias: --figures; default: True if --csv True)
//!   --no-charts               Skip generating .png charts/graphs (alias: --no-figures)
//!   --json <PAYLOAD_OR_FILE>  Score VA data from JSON string, .json file path, or `-` (stdin)
//!   --format <FMT>            Output format: dir | json | json-extended | protobuf | protobuf-extended
//!   --out-json                Shorthand for `--format json`
//!   --out-protobuf            Shorthand for `--format protobuf`
//!   --extended                Include cohort CSMFs, age groupings, and endorsement rates in JSON/Protobuf
//!   --output-file <PATH>      Write JSON or Protobuf response to <PATH> (or `-` for stdout; default: `-`)
//!   --language <LANG>         english | chinese | spanish (default: english)
//!   --legacy-format           Output files in v1.1 directory format
//!   --list-countries          List all valid country codes and exit
//!   --version                 Print version and exit
//! ```

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

use smartva_rust::{SmartVaEngine, SmartVaRunOptions};

const VERSION_STR: &str = smartva_rust::VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Dir,
    Json,
    Protobuf,
}

fn parse_bool_flag(name: &str, raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "1" => Ok(true),
        "false" | "f" | "no" | "n" | "0" => Ok(false),
        _ => Err(format!("Invalid boolean value for {name}: {raw:?}")),
    }
}

fn print_help(prog: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(
        out,
        "{VERSION_STR}\n\
         Drop-in Pure-Rust replacement for IHME SmartVA-Analyze (`smartva` / `va_cli.py`).\n\n\
         USAGE:\n  \
           {prog} [OPTIONS] <INPUT_CSV> <OUTPUT_DIR>\n  \
           {prog} [OPTIONS] --json <FILE_OR_JSON_OR_STDIN> [--format <FMT>] [--output-file <PATH>]\n  \
           {prog} <COMMAND>\n\n\
         COMMANDS:\n  \
           help, -h, --help          Show this help message and exit\n  \
           version, -V, --version    Show program's version number and exit\n  \
           countries, --list-countries\n                            \
                                     List all valid ISO-3 country codes and exit\n\n\
         POSITIONAL ARGUMENTS:\n  \
           <INPUT_CSV>               Input Verbal Autopsy CSV file (PHMRC Full/Short, WHO 2016, or WHO 2022)\n  \
           <OUTPUT_DIR>              Output directory for 4-folder CSV, .xlsx, and .png results\n\n\
         OPTIONS:\n  \
           --country <CODE>          Data origin country ISO-3 abbreviation (default: \"Unknown\")\n  \
           --hiv <BOOL>              Data originates from an HIV region (default: True)\n  \
           --malaria <BOOL>          Data originates from a Malaria region (default: True)\n  \
           --hce <BOOL>              Use Health Care Experience (HCE) variables (default: True)\n  \
           --freetext <BOOL>         Use \"open response\" free-text variables (default: True)\n  \
           --csv <BOOL>              Generate 4-folder CSV + .xlsx directory output (default: True with OUTPUT_DIR)\n  \
           --no-csv                  Skip generating CSV + .xlsx directory files\n  \
           --charts <BOOL>           Generate .png figures and graphs (alias: --figures; default: True)\n  \
           --no-charts               Skip generating .png figures and graphs (alias: --no-figures)\n  \
           --json <FILE_OR_JSON>     Accept input VA data as JSON (inline JSON, file path, or `-` for stdin)\n  \
           --format <FMT>            Output format: dir | json | json-extended | protobuf | protobuf-extended\n  \
           --out-json                Shorthand for `--format json`\n  \
           --out-protobuf            Shorthand for `--format protobuf`\n  \
           --extended                Include cohort CSMFs and population CSV tables in JSON/Protobuf output\n  \
           --output-file <PATH>      File path for JSON/Protobuf output (`-` for stdout; default: `-`)\n  \
           --language <LANG>         Translation language: english | chinese | spanish (default: english)\n  \
           --legacy-format           Output files in a file structure similar to v1.1\n  \
           --list-countries          List all valid country codes and exit\n  \
           -V, --version             Show program's version number and exit\n  \
           -h, --help                Show this help message and exit\n\n\
         EXAMPLES:\n  \
           # 1. Standard 4-folder CSV + .xlsx + .png cohort analysis (drop-in for `smartva` CLI):\n  \
           {prog} --country IND --hiv true --malaria true input_va.csv ./output_dir\n\n  \
           # 2. Fast batch scoring without .png chart rendering:\n  \
           {prog} --country UNK --no-charts input_va.csv ./output_dir\n\n  \
           # 3. Score a single VA or batch from inline JSON and emit extended JSON to stdout:\n  \
           {prog} --country IND --json '{{\"sid\":\"VA-01\",\"Id10019\":\"male\",\"ageInYears\":62,\"Id10147\":\"yes\"}}' --format json-extended\n\n  \
           # 4. Score an ODK CSV and emit Protocol Buffers v3 binary output:\n  \
           {prog} --country IND --no-csv --format protobuf-extended --output-file results.pb input_va.csv"
    );
}

struct CliState {
    opts: SmartVaRunOptions,
    csv_explicit: Option<bool>,
    charts_explicit: Option<bool>,
    json_input: Option<String>,
    format: OutputFormat,
    output_file: Option<String>,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| s.as_str()).unwrap_or("smartva-rs");

    let default_opts = SmartVaRunOptions {
        country: "Unknown".to_string(),
        ..Default::default()
    };

    let mut state = CliState {
        opts: default_opts,
        csv_explicit: None,
        charts_explicit: None,
        json_input: None,
        format: OutputFormat::Dir,
        output_file: None,
    };

    let mut list_countries = false;
    let mut positionals: Vec<String> = Vec::new();

    if args.len() == 1 {
        print_help(prog);
        process::exit(0);
    }

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-h" || arg == "--help" || (i == 1 && arg.eq_ignore_ascii_case("help")) {
            print_help(prog);
            process::exit(0);
        } else if arg == "--version"
            || arg == "-V"
            || arg == "-v"
            || (i == 1 && arg.eq_ignore_ascii_case("version"))
        {
            let _ = writeln!(io::stdout().lock(), "{VERSION_STR}");
            process::exit(0);
        } else if arg == "--list-countries"
            || (i == 1 && (arg.eq_ignore_ascii_case("countries") || arg.eq_ignore_ascii_case("list-countries")))
        {
            list_countries = true;
            i += 1;
        } else if arg == "--legacy-format" {
            state.opts.legacy_format = true;
            i += 1;
        } else if arg == "--no-csv" {
            state.csv_explicit = Some(false);
            i += 1;
        } else if arg == "--no-charts" || arg == "--no-figures" {
            state.charts_explicit = Some(false);
            i += 1;
        } else if arg == "--out-json" {
            state.format = OutputFormat::Json;
            i += 1;
        } else if arg == "--out-protobuf" || arg == "--out-proto" {
            state.format = OutputFormat::Protobuf;
            i += 1;
        } else if arg == "--extended" {
            state.opts.extended = true;
            i += 1;
        } else if let Some((flag, val)) = arg.split_once('=') {
            apply_kv_flag(flag, val, &mut state);
            i += 1;
        } else if matches!(
            arg.as_str(),
            "--country"
                | "--hiv"
                | "--malaria"
                | "--hce"
                | "--freetext"
                | "--figures"
                | "--charts"
                | "--csv"
                | "--json"
                | "--format"
                | "--output-file"
                | "--language"
        ) {
            if i + 1 >= args.len() {
                eprintln!("Error: {arg} requires a value");
                process::exit(2);
            }
            apply_kv_flag(arg, &args[i + 1], &mut state);
            i += 2;
        } else if arg.starts_with('-') {
            eprintln!("Error: unrecognized option {arg}");
            process::exit(2);
        } else {
            positionals.push(arg.clone());
            i += 1;
        }
    }

    let engine = SmartVaEngine::new();

    if list_countries {
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "\nAvailable country codes:");
        for c in engine.list_countries() {
            if writeln!(out, "- {c}").is_err() {
                break;
            }
        }
        process::exit(0);
    }

    let result = if let Some(json_arg) = &state.json_input {
        if state.format == OutputFormat::Dir && positionals.is_empty() {
            state.format = OutputFormat::Json;
        }
        let payload = read_json_input(json_arg).unwrap_or_else(|e| {
            eprintln!("Error reading --json input: {e}");
            process::exit(2);
        });
        engine
            .score_json_result(&payload, &state.opts)
            .map(|(res, ext)| {
                state.opts.extended = ext;
                res
            })
    } else {
        if positionals.is_empty() {
            eprintln!("Error: INPUT_CSV (or --json <PAYLOAD>) must be specified.");
            print_help(prog);
            process::exit(2);
        }
        let input_csv = PathBuf::from(&positionals[0]);
        let output_dir_opt = positionals.get(1).map(PathBuf::from);

        let write_csv = state
            .csv_explicit
            .unwrap_or_else(|| output_dir_opt.is_some() && state.format == OutputFormat::Dir);
        let write_charts = state
            .charts_explicit
            .unwrap_or_else(|| write_csv && output_dir_opt.is_some());

        state.opts.write_csv = write_csv;
        state.opts.figures = write_charts;

        if (write_csv || write_charts) && output_dir_opt.is_none() {
            eprintln!("Error: OUTPUT_DIR must be specified when --csv or --charts is enabled.");
            print_help(prog);
            process::exit(2);
        }

        if !write_csv && !write_charts && state.format == OutputFormat::Dir {
            state.format = OutputFormat::Json;
        }

        if let Some(output_dir) = output_dir_opt {
            if write_csv || write_charts {
                engine.run_cli_directory(&input_csv, &output_dir, &state.opts)
            } else {
                engine.score_csv(&input_csv, &state.opts)
            }
        } else {
            engine.score_csv(&input_csv, &state.opts)
        }
    };

    let res = match result {
        Ok(r) => r,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };

    match state.format {
        OutputFormat::Dir => {}
        OutputFormat::Json => {
            let json_str = engine.to_json(&res, state.opts.extended);
            write_output_bytes(state.output_file.as_deref(), json_str.as_bytes(), true);
        }
        OutputFormat::Protobuf => {
            let proto_bytes = engine.to_protobuf(&res, state.opts.extended);
            write_output_bytes(state.output_file.as_deref(), &proto_bytes, false);
        }
    }
}

fn read_json_input(arg: &str) -> Result<String, String> {
    let trimmed = arg.trim();
    if trimmed == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| e.to_string())?;
        return Ok(buf);
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return Ok(trimmed.to_string());
    }
    fs::read_to_string(trimmed).map_err(|e| format!("{trimmed}: {e}"))
}

fn write_output_bytes(output_file: Option<&str>, bytes: &[u8], append_newline: bool) {
    match output_file {
        Some(path) if path != "-" => {
            if let Err(e) = fs::write(path, bytes) {
                eprintln!("Error writing output to {path}: {e}");
                process::exit(2);
            }
        }
        _ => {
            let mut stdout = io::stdout().lock();
            if let Err(e) = stdout.write_all(bytes) {
                eprintln!("Error writing to stdout: {e}");
                process::exit(2);
            }
            if append_newline {
                let _ = stdout.write_all(b"\n");
            }
            let _ = stdout.flush();
        }
    }
}

fn apply_kv_flag(flag: &str, val: &str, state: &mut CliState) {
    match flag {
        "--country" => state.opts.country = val.to_string(),
        "--hiv" => {
            state.opts.hiv = parse_bool_flag(flag, val).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(2);
            })
        }
        "--malaria" => {
            state.opts.malaria = parse_bool_flag(flag, val).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(2);
            })
        }
        "--hce" => {
            state.opts.hce = parse_bool_flag(flag, val).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(2);
            })
        }
        "--freetext" => {
            state.opts.freetext = parse_bool_flag(flag, val).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(2);
            })
        }
        "--csv" => {
            let b = parse_bool_flag(flag, val).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(2);
            });
            state.csv_explicit = Some(b);
        }
        "--figures" | "--charts" => {
            let b = parse_bool_flag(flag, val).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(2);
            });
            state.charts_explicit = Some(b);
        }
        "--json" => state.json_input = Some(val.to_string()),
        "--format" => match val.trim().to_ascii_lowercase().as_str() {
            "dir" | "directory" | "csv" => state.format = OutputFormat::Dir,
            "json" | "json-basic" | "json-standard" => {
                state.format = OutputFormat::Json;
            }
            "json-extended" => {
                state.format = OutputFormat::Json;
                state.opts.extended = true;
            }
            "protobuf" | "proto" | "pb" => {
                state.format = OutputFormat::Protobuf;
            }
            "protobuf-extended" | "proto-extended" | "pb-extended" => {
                state.format = OutputFormat::Protobuf;
                state.opts.extended = true;
            }
            other => {
                eprintln!(
                    "Error: invalid --format {other:?} (expected dir, json, json-extended, protobuf, or protobuf-extended)"
                );
                process::exit(2);
            }
        },
        "--output-file" => state.output_file = Some(val.to_string()),
        "--language" => state.opts.language = val.to_ascii_lowercase(),
        _ => {
            eprintln!("Error: unrecognized option {flag}");
            process::exit(2);
        }
    }
}
