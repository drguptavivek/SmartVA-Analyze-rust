//! Pure-Rust SmartVA domain data modules (`smartva/data/*.rs`) and CSV assets (`smartva/data/*.csv`).
//! Mirrors the upstream Python `smartva/data/` package 1-for-1 without Python or JSON conversion.

pub mod adult_pre_symptom_data;
pub mod adult_symptom_data;
pub mod adult_tariff_data;
pub mod answer_ranges;
pub mod child_pre_symptom_data;
pub mod child_symptom_data;
pub mod child_tariff_data;
pub mod codebook;
pub mod common_data;
pub mod constants;
pub mod countries;
pub mod default_fill_data;
pub mod gbd_causes;
pub mod icds;
pub mod neonate_pre_symptom_data;
pub mod neonate_symptom_data;
pub mod neonate_tariff_data;
pub mod who_data;
pub mod word_conversions;

#[derive(Debug, Clone, Copy)]
pub enum BinaryConversionSpec {
    Map(&'static [(i32, &'static str)]),
    List(&'static [i32]),
    Ldap(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub enum RecodeTarget {
    Column(&'static str),
    Literal(i32),
}

/// Embedded `tariffs-{adult,child,neonate}.csv` baked into the Rust binary at compile time.
pub fn embedded_tariffs_csv(module: &str) -> Option<&'static str> {
    match module {
        "adult" => Some(include_str!("tariffs-adult.csv")),
        "child" => Some(include_str!("tariffs-child.csv")),
        "neonate" => Some(include_str!("tariffs-neonate.csv")),
        _ => None,
    }
}

/// Embedded `validated-{adult,child,neonate}.csv` baked into the Rust binary at compile time.
pub fn embedded_validated_csv(module: &str) -> Option<&'static str> {
    match module {
        "adult" => Some(include_str!("validated-adult.csv")),
        "child" => Some(include_str!("validated-child.csv")),
        "neonate" => Some(include_str!("validated-neonate.csv")),
        _ => None,
    }
}

/// Embedded `{adult,child,neonate}_undetermined_weights.csv` baked into the Rust binary at compile time.
pub fn embedded_undetermined_weights_csv(module: &str) -> Option<&'static str> {
    match module {
        "adult" => Some(include_str!("adult_undetermined_weights.csv")),
        "child" => Some(include_str!("child_undetermined_weights.csv")),
        "neonate" => Some(include_str!("neonate_undetermined_weights.csv")),
        _ => None,
    }
}

/// Embedded `SmartVA Analyze Output Interpretation Sheet.docx` baked into the Rust binary at compile time.
pub const EMBEDDED_INTERPRETATION_DOCX: &[u8] =
    include_bytes!("SmartVA Analyze Output Interpretation Sheet.docx");
