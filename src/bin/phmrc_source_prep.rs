//! Standalone CLI binary (`phmrc-source-prep`) to convert GHDx PHMRC Gold Standard
//! Verbal Autopsy validation CSVs (`IHME_PHMRC_VA_DATA_{ADULT,CHILD,NEONATE}_Y2013M09D11_0.csv`)
//! into standard ODK PHMRC Full Questionnaire CSVs for `SmartVA-Analyze` and `smartva-rs`.

use std::env;
use std::path::Path;
use std::process;

use smartva_rust::phmrc_source_prep::convert_phmrc_gold_standard_csv;

fn version_str() -> String {
    format!(
        "phmrc-source-prep {} (SmartVA-Analyze {}-rust)",
        env!("CARGO_PKG_VERSION"),
        smartva_rust::SMARTVA_UPSTREAM_VERSION
    )
}

fn print_help(prog: &str) {
    let version = version_str();
    println!(
        "{version}\n\
         Converts GHDx PHMRC Gold Standard Verbal Autopsy validation CSVs into\n\
         standard ODK PHMRC Full Questionnaire CSVs accepted by SmartVA-Analyze and smartva-rs.\n\n\
         USAGE:\n  \
           {prog} <INPUT_PHMRC_GS_CSV> <OUTPUT_ODK_CSV>\n  \
           {prog} <COMMAND>\n\n\
         COMMANDS:\n  \
           help, -h, --help          Show this help message and exit\n  \
           version, -V, --version    Show program's version number and exit\n\n\
         ARGUMENTS:\n  \
           <INPUT_PHMRC_GS_CSV>      Input GHDx PHMRC Gold Standard CSV file\n                            \
                                     (Adult, Child, or Neonate module is auto-detected from headers/values)\n  \
           <OUTPUT_ODK_CSV>          Output ODK PHMRC Full Instrument CSV file\n\n\
         EXAMPLES:\n  \
           {prog} IHME_PHMRC_VA_DATA_ADULT_Y2013M09D11_0.csv adult_odk.csv\n  \
           {prog} IHME_PHMRC_VA_DATA_CHILD_Y2013M09D11_0.csv child_odk.csv\n  \
           {prog} IHME_PHMRC_VA_DATA_NEONATE_Y2013M09D11_0.csv neonate_odk.csv"
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args
        .first()
        .map(|s| s.as_str())
        .unwrap_or("phmrc-source-prep");

    if args.len() == 1
        || args[1] == "-h"
        || args[1] == "--help"
        || args[1].eq_ignore_ascii_case("help")
    {
        print_help(prog);
        process::exit(0);
    }
    if args[1] == "-V"
        || args[1] == "-v"
        || args[1] == "--version"
        || args[1].eq_ignore_ascii_case("version")
    {
        println!("{}", version_str());
        process::exit(0);
    }
    if args.len() != 3 {
        print_help(prog);
        process::exit(2);
    }
    let input_path = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);
    match convert_phmrc_gold_standard_csv(input_path, output_path) {
        Ok(rows) => {
            println!(
                "Converted {} PHMRC validation rows: {} -> {}",
                rows,
                input_path.display(),
                output_path.display()
            );
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
