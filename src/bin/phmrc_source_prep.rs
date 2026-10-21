//! Standalone CLI binary (`phmrc-source-prep`) to convert GHDx PHMRC Gold Standard
//! Verbal Autopsy validation CSVs (`IHME_PHMRC_VA_DATA_{ADULT,CHILD,NEONATE}_Y2013M09D11_0.csv`)
//! into standard ODK PHMRC Full Questionnaire CSVs for `SmartVA-Analyze` and `smartva-rs`.

use std::env;
use std::path::Path;
use std::process;

use smartva_rust::phmrc_source_prep::convert_phmrc_gold_standard_csv;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 || args[1] == "-h" || args[1] == "--help" {
        eprintln!(
            "Usage: phmrc-source-prep <INPUT_PHMRC_GS_CSV> <OUTPUT_ODK_CSV>\n\n\
             Converts a GHDx PHMRC Gold Standard validation CSV\n\
             (e.g. IHME_PHMRC_VA_DATA_ADULT_Y2013M09D11_0.csv) into an ODK PHMRC Full\n\
             Questionnaire CSV accepted by both upstream Python SmartVA-Analyze and smartva-rs."
        );
        process::exit(if args.len() == 2 { 0 } else { 2 });
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
