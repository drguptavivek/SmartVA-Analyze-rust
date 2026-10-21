//! Pure-Rust SmartVA-Analyze Pipeline Engine (`WHOPrep` -> `CommonPrep` -> `PreSymptomPrep`
//! -> `RulesPrep` -> `SymptomPrep` -> `TariffPrep` -> `OutputPrep`).
//!
//! Consumes the SmartVA CSV files (`tariffs-{module}.csv`, `validated-{module}.csv`,
//! `{module}_undetermined_weights.csv`) and compiled Rust data modules (`crate::data::*`)
//! directly without any Python or JSON intermediate conversion.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Datelike;

use crate::charts;
use crate::data::{
    self, adult_pre_symptom_data, adult_symptom_data, adult_tariff_data, answer_ranges,
    child_pre_symptom_data, child_symptom_data, child_tariff_data, codebook, common_data,
    gbd_causes, icds, neonate_pre_symptom_data, neonate_symptom_data, neonate_tariff_data,
    who_data, BinaryConversionSpec, RecodeTarget,
};
use crate::ldap::{eval_ldap, evaluate_module_rules};
use crate::stemmer::stem_porter2;
use crate::xlsx;
use crate::{
    SmartVaAgeGroupingRow, SmartVaCausesOfDeathRow, SmartVaCliDirectoryResult, SmartVaCsmfRow,
    SmartVaError, SmartVaGbdCsmfRow, SmartVaPrediction, SmartVaRunOptions,
};

const MODULES: [&str; 3] = ["adult", "child", "neonate"];

pub type Row = HashMap<String, String>;

/// Format a floating-point number to match Python 3's `str(float)` representation.
pub fn py_float_str(val: f64) -> String {
    let v = if val == 0.0 { 0.0 } else { val };
    let s = format!("{}", v);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        format!("{}.0", s)
    } else {
        s
    }
}

pub fn safe_int(val: Option<&str>) -> i32 {
    let Some(s) = val else { return 0 };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return 0;
    }
    if let Ok(i) = trimmed.parse::<i32>() {
        return i;
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        if f.is_finite() {
            return f as i32;
        }
    }
    0
}

pub fn safe_float(val: Option<&str>) -> f64 {
    let Some(s) = val else { return 0.0 };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    trimmed.parse::<f64>().unwrap_or(0.0)
}

fn parse_py_int(val: Option<&str>) -> Option<i32> {
    let s = val?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<i32>().ok()
}

fn parse_py_float(val: Option<&str>) -> Option<f64> {
    let s = val?.trim();
    if s.is_empty() {
        return None;
    }
    let f = s.parse::<f64>().ok()?;
    if f.is_finite() {
        Some(f)
    } else {
        None
    }
}

/// Resolve the SmartVA `data/` directory containing `tariffs-*.csv`, `validated-*.csv`, and `*_undetermined_weights.csv`.
pub fn resolve_data_dir() -> PathBuf {
    let manifest_data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("data");
    if manifest_data.join("tariffs-adult.csv").is_file() {
        return manifest_data;
    }
    let parent_py_data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("src")
        .join("smartva")
        .join("data");
    if parent_py_data.join("tariffs-adult.csv").is_file() {
        return parent_py_data;
    }
    manifest_data
}

#[derive(Default)]
struct ReportLoggers {
    report_lines: Vec<String>,
    sids: Vec<String>,
    refused: Vec<String>,
    valid_consent: Vec<String>,
    valid_age: Vec<String>,
    prediction: Vec<String>,
    warnings: Vec<String>,
}

fn format_header_name(header: &str) -> String {
    let s = header.replace([':', '/'], "-");
    s.split('-').next_back().unwrap_or("").to_string()
}

fn read_csv_matrix(path: &Path) -> Result<(Vec<String>, Vec<Row>), SmartVaError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|e| SmartVaError::IoError(format!("Failed to read CSV {}: {}", path.display(), e)))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut matrix = Vec::new();
    for result in rdr.records() {
        let record = result.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        let mut row = Row::with_capacity(headers.len());
        for (i, h) in headers.iter().enumerate() {
            let val = record.get(i).unwrap_or("").to_string();
            row.insert(h.clone(), val);
        }
        matrix.push(row);
    }
    Ok((headers, matrix))
}

fn write_csv_matrix(path: &Path, headers: &[String], matrix: &[Row]) -> Result<(), SmartVaError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| SmartVaError::IoError(e.to_string()))?;
    }
    let file = File::create(path)
        .map_err(|e| SmartVaError::IoError(format!("Failed to create {}: {}", path.display(), e)))?;
    let mut wtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(BufWriter::new(file));
    wtr.write_record(headers)
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    let mut record = Vec::with_capacity(headers.len());
    for row in matrix {
        record.clear();
        for h in headers {
            record.push(row.get(h).map(|s| s.as_str()).unwrap_or(""));
        }
        wtr.write_record(&record)
            .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    }
    wtr.flush()
        .map_err(|e| SmartVaError::IoError(e.to_string()))?;
    Ok(())
}

fn write_csv_rows(path: &Path, rows: &[Vec<String>]) -> Result<(), SmartVaError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| SmartVaError::IoError(e.to_string()))?;
    }
    let file = File::create(path)
        .map_err(|e| SmartVaError::IoError(format!("Failed to create {}: {}", path.display(), e)))?;
    let mut wtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(BufWriter::new(file));
    for row in rows {
        wtr.write_record(row)
            .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    }
    wtr.flush()
        .map_err(|e| SmartVaError::IoError(e.to_string()))?;
    Ok(())
}

fn format_headers_step(
    source_path: &Path,
    dest_path: &Path,
    loggers: &mut ReportLoggers,
) -> Result<Vec<String>, SmartVaError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(source_path)
        .map_err(|e| SmartVaError::IoError(format!("Failed to open {}: {}", source_path.display(), e)))?;

    let mut records = rdr.records();
    let Some(first_res) = records.next() else {
        return Err(SmartVaError::ExecutionError(format!(
            "Source file \"{}\" does not contain data.",
            source_path.display()
        )));
    };
    let raw_headers = first_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    let Some(second_res) = records.next() else {
        return Err(SmartVaError::ExecutionError(format!(
            "Source file \"{}\" does not contain data.",
            source_path.display()
        )));
    };
    let first_data_row = second_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;

    let headers: Vec<String> = raw_headers
        .iter()
        .map(|h| format_header_name(h))
        .collect();

    // Check duplicate headers in order of appearance
    let mut seen = HashSet::new();
    let mut dupes = Vec::new();
    for h in &headers {
        if !seen.insert(h.clone()) && !dupes.contains(h) {
            dupes.push(h.clone());
        }
    }
    if !dupes.is_empty() {
        loggers.warnings.push(format!(
            "Duplicate headers found: {:?}. Please review the input file for conflicts.",
            dupes
        ));
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent).map_err(|e| SmartVaError::IoError(e.to_string()))?;
    }
    let file = File::create(dest_path).map_err(|e| SmartVaError::IoError(e.to_string()))?;
    let mut wtr = csv::WriterBuilder::new()
        .terminator(csv::Terminator::CRLF)
        .from_writer(BufWriter::new(file));

    wtr.write_record(&headers)
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?;

    wtr.write_record(&first_data_row)
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?;

    for rec_res in records {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        wtr.write_record(&rec)
            .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    }
    wtr.flush()
        .map_err(|e| SmartVaError::IoError(e.to_string()))?;

    Ok(headers)
}

fn is_who_header(h: &str) -> bool {
    if h.starts_with("age") || h.starts_with("is") {
        return true;
    }
    let bytes = h.as_bytes();
    if bytes.len() >= 3 && (bytes[0] == b'I' || bytes[0] == b'i') && bytes[1] == b'd' {
        let mut i = 2;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i > 2 {
            if i == bytes.len() {
                return true;
            }
            if bytes[i] == b'_' && i + 1 < bytes.len() {
                return bytes[i + 1..]
                    .iter()
                    .all(|&b| b.is_ascii_alphanumeric() || b == b'_');
            }
        }
    }
    false
}

fn who_questionnaire_test(headers: &[String]) -> bool {
    if headers.is_empty() {
        return false;
    }
    let matches = headers.iter().filter(|h| is_who_header(h)).count();
    (matches as f64) / (headers.len() as f64) > 0.80
        || headers
            .iter()
            .any(|h| h == "Id10013" || h == "ageInYears" || h == "ageInDays")
}

fn project_matrix_to_headers(headers: &[String], matrix: &mut [Row]) {
    let keep: HashSet<&str> = headers.iter().map(|s| s.as_str()).collect();
    for row in matrix {
        row.retain(|k, _| keep.contains(k.as_str()));
        for h in headers {
            if !row.contains_key(h) {
                row.insert(h.clone(), String::new());
            }
        }
        row.shrink_to_fit();
    }
}

fn who_prep_in_memory(matrix: &mut [Row], who_2016: bool) -> Vec<String> {
    let mut headers_set: BTreeSet<String> = BTreeSet::new();
    for &h in who_data::ADDITIONAL_HEADERS {
        headers_set.insert(h.to_string());
    }
    for &(k, _) in who_data::YES_NO_QUESTIONS {
        headers_set.insert(k.to_string());
    }
    for &((h, _), _) in who_data::RECODE_QUESTIONS {
        headers_set.insert(h.to_string());
    }
    for &(k, _) in who_data::RENAME_QUESTIONS {
        headers_set.insert(k.to_string());
    }
    for &(k, _) in who_data::REVERSE_ONE_HOT_MULTISELECT {
        headers_set.insert(k.to_string());
    }
    for &((h, _), _) in who_data::RECODE_MULTISELECT {
        headers_set.insert(h.to_string());
    }
    for &(k, _) in who_data::ONE_HOT_FROM_MULTISELECT {
        headers_set.insert(k.to_string());
    }
    for &(k, _) in who_data::UNIT_IF_AMOUNT {
        headers_set.insert(k.to_string());
    }
    for &((ucol, vcol, _), _) in who_data::DURATION_CONVERSIONS {
        headers_set.insert(ucol.to_string());
        headers_set.insert(vcol.to_string());
    }

    for row in matrix.iter_mut() {
        // 1. determine_consent
        if let Some(v) = row.get("Id10013") {
            let c = if v == "yes" { "1" } else { "0" };
            row.insert("gen_3_1".to_string(), c.to_string());
        }
        // 2. calculate_age
        who_calculate_age(row);
        // 3. recode_yes_no_questions
        for &(dest, mut src) in who_data::YES_NO_QUESTIONS {
            if !who_2016 {
                if let Some(&(_, override_src)) = who_data::YES_NO_QUESTIONS_WHO_2022
                    .iter()
                    .find(|&&(k, _)| k == dest)
                {
                    src = override_src;
                }
            }
            let val = match row.get(src).map(|s| s.as_str()) {
                Some("yes") => "1",
                Some("no") => "0",
                Some("ref") => "8",
                Some("dk") => "9",
                _ => "",
            };
            row.insert(dest.to_string(), val.to_string());
        }
        // 4. recode_categoricals
        for &((dest, src), mapping) in who_data::RECODE_QUESTIONS {
            let val = row
                .get(src)
                .and_then(|s| mapping.iter().find(|&&(k, _)| k == s.as_str()))
                .map(|&(_, v)| v.to_string())
                .unwrap_or_default();
            row.insert(dest.to_string(), val);
        }
        // 5. rename_questions
        for &(dest, src) in who_data::RENAME_QUESTIONS {
            let val = row.get(src).cloned().unwrap_or_default();
            row.insert(dest.to_string(), val);
        }
        // 6. reverse_one_hot_multiselect
        for &(dest, mapping) in who_data::REVERSE_ONE_HOT_MULTISELECT {
            let mut endorsement = BTreeSet::new();
            for &(src, value) in mapping {
                if row.get(src).map(|s| s.as_str()) == Some("yes") {
                    endorsement.insert(value);
                }
            }
            let joined = endorsement
                .into_iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            row.insert(dest.to_string(), joined);
        }
        // 7. recode_multiselects
        for &((dest, src), mapping) in who_data::RECODE_MULTISELECT {
            let mut vals: Vec<i32> = row
                .get(src)
                .map(|s| s.as_str())
                .unwrap_or("")
                .split_whitespace()
                .filter_map(|tok| mapping.iter().find(|&&(k, _)| k == tok).map(|&(_, v)| v))
                .collect();
            vals.sort_unstable();
            let joined = vals
                .into_iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            row.insert(dest.to_string(), joined);
        }
        // 8. encode_one_hot_from_multiselect
        for &(dest, (src, choice)) in who_data::ONE_HOT_FROM_MULTISELECT {
            let val = if let Some(s) = row.get(src) {
                let present = s.split_whitespace().any(|w| w == choice);
                if present { "1" } else { "0" }
            } else {
                ""
            };
            row.insert(dest.to_string(), val.to_string());
        }
        // 9. encode_multiple_from_combined
        if !who_2016 {
            for &(src, (dest1, dest2)) in who_data::COMBINED_TO_MULTIPLE_WHO_2022 {
                if row.get(src).map(|s| s.as_str()) == Some("yes") {
                    row.insert(dest1.to_string(), "1".to_string());
                    row.insert(dest2.to_string(), "1".to_string());
                }
            }
        }
        // 10. map_units_from_values
        for &(dest, unit_entries) in who_data::UNIT_IF_AMOUNT {
            let mut found = String::new();
            for &(src, unit) in unit_entries {
                if safe_int(row.get(src).map(|s| s.as_str())) > 0 {
                    found = unit.to_string();
                    break;
                }
            }
            row.insert(dest.to_string(), found);
        }
        // 11. convert_durations
        for &((ucol, vcol, unit), entries) in who_data::DURATION_CONVERSIONS {
            let mut matched = None;
            for &(src, scalar) in entries {
                let value = safe_int(row.get(src).map(|s| s.as_str())) * scalar;
                if value > 0 {
                    matched = Some((unit.to_string(), value.to_string()));
                    break;
                }
            }
            if let Some((u, v)) = matched {
                row.insert(ucol.to_string(), u);
                row.insert(vcol.to_string(), v);
            } else {
                row.insert(ucol.to_string(), String::new());
                row.insert(vcol.to_string(), String::new());
            }
        }
        // 12..19 custom WHO mappings
        who_map_adult_chest_pain_duration(row);
        who_map_adult_pregnancy_duration(row);
        who_map_child_illness_duration(row);
        who_map_neonate_first_cry(row);
        who_map_child_unconsciousness_start(row);
        who_map_neonate_delivery_type(row);
        who_map_child_birth_size(row);
        who_map_redundant_child_age_data(row);
    }

    let sorted_headers: Vec<String> = headers_set.into_iter().collect();
    project_matrix_to_headers(&sorted_headers, matrix);
    sorted_headers
}

fn run_who_prep(cleanheaders_path: &Path, who_2016: bool) -> Result<(), SmartVaError> {
    let (_, mut matrix) = read_csv_matrix(cleanheaders_path)?;
    let sorted_headers = who_prep_in_memory(&mut matrix, who_2016);
    write_csv_matrix(cleanheaders_path, &sorted_headers, &matrix)
}

fn who_calculate_age(row: &mut Row) {
    let unit_col = "gen_5_4";
    for col in ["ageInYears", "age_adult", "age_child_years"] {
        let age_in_years = safe_int(row.get(col).map(|s| s.as_str()));
        if age_in_years >= 1 {
            row.insert(unit_col.to_string(), "1".to_string());
            row.insert("gen_5_4a".to_string(), age_in_years.to_string());
            row.insert("agedays".to_string(), (age_in_years * 365).to_string());
            return;
        }
    }
    for col in ["ageInMonths", "age_child_months"] {
        let age_in_months = safe_int(row.get(col).map(|s| s.as_str()));
        if age_in_months >= 1 {
            if age_in_months < 12 {
                row.insert(unit_col.to_string(), "2".to_string());
                row.insert("gen_5_4b".to_string(), age_in_months.to_string());
            } else {
                row.insert(unit_col.to_string(), "1".to_string());
                row.insert("gen_5_4a".to_string(), (age_in_months / 12).to_string());
            }
            row.insert("agedays".to_string(), (age_in_months * 30).to_string());
            return;
        }
    }
    for col in [
        "ageInDays",
        "ageInDaysNeonate",
        "age_neonate_days",
        "age_child_days",
    ] {
        if let Some(age_in_days) = parse_py_int(row.get(col).map(|s| s.as_str())) {
            if age_in_days <= 30 {
                row.insert(unit_col.to_string(), "4".to_string());
                row.insert("gen_5_4c".to_string(), age_in_days.to_string());
            } else if age_in_days < 365 {
                row.insert(unit_col.to_string(), "2".to_string());
                row.insert("gen_5_4b".to_string(), (age_in_days / 30).to_string());
            } else {
                row.insert(unit_col.to_string(), "1".to_string());
                row.insert("gen_5_4a".to_string(), (age_in_days / 365).to_string());
            }
            row.insert("agedays".to_string(), age_in_days.to_string());
            return;
        }
    }
    for suffix in ["", "1", "2"] {
        for (module, val) in [("Adult", "3"), ("Child", "2"), ("Neonate", "1")] {
            let key = format!("is{}{}", module, suffix);
            if safe_int(row.get(&key).map(|s| s.as_str())) != 0 {
                row.insert("gen_5_4d".to_string(), val.to_string());
                row.insert("agedays".to_string(), String::new());
                return;
            }
        }
    }
    row.insert("gen_5_4d".to_string(), "9".to_string());
    row.insert("agedays".to_string(), String::new());
}

fn who_map_adult_chest_pain_duration(row: &mut Row) {
    let key = "adult_2_44";
    if let Some(m) = parse_py_int(row.get("Id10178").map(|s| s.as_str())) {
        if (0..30).contains(&m) {
            row.insert(key.to_string(), "1".to_string());
            return;
        } else if (30..24 * 60).contains(&m) {
            row.insert(key.to_string(), "2".to_string());
            return;
        }
    }
    if let Some(h) = parse_py_int(row.get("Id10179").map(|s| s.as_str())) {
        if (1..24).contains(&h) {
            row.insert(key.to_string(), "2".to_string());
            return;
        } else if h >= 24 {
            row.insert(key.to_string(), "3".to_string());
            return;
        }
    }
    if let Some(d) = parse_py_int(row.get("Id10179_1").map(|s| s.as_str())) {
        if d > 0 {
            row.insert(key.to_string(), "3".to_string());
            return;
        }
    }
    row.insert(key.to_string(), String::new());
}

fn who_map_adult_pregnancy_duration(row: &mut Row) {
    let key = "adult_3_11a";
    if let Some(m) = parse_py_int(row.get("Id10309").map(|s| s.as_str())) {
        if (0..11).contains(&m) {
            row.insert(key.to_string(), m.to_string());
        } else {
            row.insert(key.to_string(), String::new());
        }
    }
}

fn who_map_child_illness_duration(row: &mut Row) {
    let key = "child_1_21";
    if let Some(d) = parse_py_int(row.get("Id10120_1").map(|s| s.as_str())) {
        row.insert(format!("{}a", key), d.to_string());
        row.insert(key.to_string(), "4".to_string());
        return;
    }
    if let Some(m) = parse_py_int(row.get("Id10121").map(|s| s.as_str())) {
        row.insert(format!("{}b", key), m.to_string());
        row.insert(key.to_string(), "2".to_string());
        return;
    }
    if let Some(y) = parse_py_int(row.get("Id10122").map(|s| s.as_str())) {
        row.insert(format!("{}c", key), (y * 12).to_string());
        row.insert(key.to_string(), "2".to_string());
    }
}

fn who_map_neonate_first_cry(row: &mut Row) {
    let key = "child_3_8";
    if let Some(m) = parse_py_int(row.get("Id10106").map(|s| s.as_str())) {
        let v = if (0..=5).contains(&m) {
            "1"
        } else if (6..=30).contains(&m) {
            "2"
        } else if m > 30 {
            "3"
        } else {
            ""
        };
        row.insert(key.to_string(), v.to_string());
    } else {
        let v = if row.get("Id10105").map(|s| s.as_str()) == Some("yes") {
            "4"
        } else {
            ""
        };
        row.insert(key.to_string(), v.to_string());
    }
}

fn who_map_child_unconsciousness_start(row: &mut Row) {
    let key = "child_4_27";
    let dur_in_hours = parse_py_int(row.get("Id10216_a").map(|s| s.as_str()));
    let dur_in_days = safe_int(row.get("Id10216_b").map(|s| s.as_str()));
    let Some(h) = dur_in_hours else {
        let v = if dur_in_days >= 1 { "3" } else { "" };
        row.insert(key.to_string(), v.to_string());
        return;
    };
    let v = if h >= 24 {
        "3"
    } else if (0..=5).contains(&h) {
        "1"
    } else if (6..=23).contains(&h) {
        "2"
    } else {
        ""
    };
    row.insert(key.to_string(), v.to_string());
}

fn who_map_neonate_delivery_type(row: &mut Row) {
    let key = "child_2_17";
    let v = if row.get("Id10342").map(|s| s.as_str()) == Some("yes") {
        "2"
    } else if row.get("Id10343").map(|s| s.as_str()) == Some("yes") {
        "1"
    } else if row.get("Id10344").map(|s| s.as_str()) == Some("yes") {
        "4"
    } else if row.get("Id10342").map(|s| s.as_str()) == Some("dk")
        && row.get("Id0343").map(|s| s.as_str()) == Some("dk")
        && matches!(row.get("Id10344").map(|s| s.as_str()), Some("no" | "dk"))
    {
        "3"
    } else {
        ""
    };
    row.insert(key.to_string(), v.to_string());
}

fn who_map_child_birth_size(row: &mut Row) {
    let key = "child_1_7";
    let v = if row.get("Id10364").map(|s| s.as_str()) == Some("yes") {
        "1"
    } else if row.get("Id10363").map(|s| s.as_str()) == Some("yes") {
        "2"
    } else if row.get("Id10365").map(|s| s.as_str()) == Some("yes") {
        "4"
    } else if ["Id10363", "Id10364", "Id10365"]
        .iter()
        .all(|&k| row.get(k).map(|s| s.as_str()) == Some("no"))
    {
        "3"
    } else {
        ""
    };
    row.insert(key.to_string(), v.to_string());
}

fn who_map_redundant_child_age_data(row: &mut Row) {
    let age_group_key = "child_1_26";
    let unit_key = "child_1_25";
    let age_days_str = row.get("agedays").cloned().unwrap_or_default();
    if age_days_str.is_empty() {
        let age_group = row.get("gen_5_4d").cloned().unwrap_or_default();
        let ag_val = if age_group == "1" || age_group == "2" {
            age_group
        } else {
            String::new()
        };
        row.insert(age_group_key.to_string(), ag_val);
        row.insert(unit_key.to_string(), String::new());
        return;
    }
    let Ok(age_days) = age_days_str.parse::<i32>() else {
        return;
    };
    if age_days <= 28 {
        row.insert(age_group_key.to_string(), "1".to_string());
    } else if age_days < 12 * 365 {
        row.insert(age_group_key.to_string(), "2".to_string());
    } else {
        row.insert(age_group_key.to_string(), String::new());
    }
    if age_days <= 30 {
        row.insert(unit_key.to_string(), "4".to_string());
        row.insert(format!("{}a", unit_key), age_days.to_string());
    } else if age_days < 365 {
        row.insert(unit_key.to_string(), "2".to_string());
        row.insert(
            format!("{}b", unit_key),
            ((age_days as f64 / 30.4) as i32).to_string(),
        );
    } else {
        row.insert(unit_key.to_string(), "1".to_string());
        row.insert(format!("{}c", unit_key), (age_days / 365).to_string());
    }
}

fn common_prep_in_memory(
    mut headers: Vec<String>,
    matrix: Vec<Row>,
    short_form: bool,
    loggers: &mut ReportLoggers,
) -> (Vec<String>, HashMap<&'static str, Vec<Row>>) {
    let mut additional_pairs: Vec<(String, String)> = Vec::new();
    let mut additional_seen: HashSet<String> = HashSet::new();
    for &k in common_data::ADDITIONAL_HEADERS {
        additional_pairs.push((k.to_string(), String::new()));
        additional_seen.insert(k.to_string());
    }
    if short_form {
        for &(k, v) in common_data::SHORT_FORM_ADDITIONAL_HEADERS_DATA {
            if !additional_seen.contains(k) {
                additional_pairs.push((k.to_string(), v.to_string()));
                additional_seen.insert(k.to_string());
            } else if let Some(pos) = additional_pairs.iter().position(|(ek, _)| ek == k) {
                additional_pairs[pos].1 = v.to_string();
            }
        }
    }

    let existing_headers: HashSet<String> = headers.iter().cloned().collect();
    let mut add_headers = Vec::new();
    let mut add_values = Vec::new();
    for (k, v) in additional_pairs {
        if !existing_headers.contains(&k) {
            add_headers.push(k);
            add_values.push(v);
        }
    }
    headers.extend(add_headers.iter().cloned());
    if !headers.iter().any(|h| h == "child_1_8a") {
        headers.push("child_1_8a".to_string());
    }

    let word_subs: HashMap<&str, &str> = common_data::WORD_SUBS.iter().copied().collect();

    let mut matrix_data: HashMap<&'static str, Vec<Row>> = HashMap::from([
        ("adult", Vec::new()),
        ("child", Vec::new()),
        ("neonate", Vec::new()),
        ("invalid-age", Vec::new()),
    ]);
    let mut sids_seen: HashSet<String> = HashSet::new();

    for (index, mut row) in matrix.into_iter().enumerate() {
        // check_sids
        if let Some(sid) = row.get("sid").cloned() {
            if sid.is_empty() {
                loggers
                    .sids
                    .push(format!("Row {} has a missing value for sid", index + 1));
            } else if sids_seen.contains(&sid) {
                loggers.sids.push(format!(
                    "Row {} has a duplicate SID: \"{}\"",
                    index + 1,
                    sid
                ));
            }
            sids_seen.insert(sid);
        }

        // check_consent
        if let Some(consent_val) = row.get(common_data::CONSENT_HEADER).cloned() {
            let sid_disp = row
                .get("sid")
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string());
            if consent_val.is_empty() || consent_val == "1" {
                // valid consent
            } else if consent_val == "0" {
                loggers.refused.push(format!(
                    "SID: {} (row {}) refused the survey",
                    sid_disp, index
                ));
                loggers.warnings.push(format!(
                    "SID: {} Refused consent.",
                    row.get("sid").map(|s| s.as_str()).unwrap_or("")
                ));
                continue;
            } else {
                let msg = format!(
                    "SID: {} ({}) Invalid value for consent: {}",
                    sid_disp, index, consent_val
                );
                loggers.warnings.push(msg.clone());
                loggers.valid_consent.push(msg);
                loggers.warnings.push(format!(
                    "SID: {} Refused consent.",
                    row.get("sid").map(|s| s.as_str()).unwrap_or("")
                ));
                continue;
            }
        }

        // expand_row (only insert non-empty defaults and AGE_VARS to avoid ~1,600 empty String allocations per row)
        for (k, v) in add_headers.iter().zip(add_values.iter()) {
            if !v.is_empty() || common_data::AGE_VARS.iter().any(|&(_, av)| av == k.as_str()) {
                if !row.contains_key(k) {
                    row.insert(k.clone(), v.clone());
                }
            }
        }

        // correct_missing_age
        let all_age_blank = common_data::AGE_VARS.iter().all(|&(_, var)| {
            row.get(var).map(|s| s.as_str()).unwrap_or("").is_empty()
        });
        if all_age_blank {
            row.insert("gen_5_4d".to_string(), "9".to_string());
        }

        // convert_cell_to_int for AGE_VARS
        let mut missing_age_key = false;
        for &(_, var) in common_data::AGE_VARS {
            if let Some(existing) = row.get(var).cloned() {
                let iv = existing.parse::<i32>().unwrap_or(0);
                row.insert(var.to_string(), iv.to_string());
            } else {
                missing_age_key = true;
                break;
            }
        }
        if missing_age_key {
            continue;
        }

        // process_multiselect_vars
        for &(hdr, mapping) in common_data::BINARY_CONVERSION_MAP {
            let raw = row.get(hdr).map(|s| s.as_str()).unwrap_or("");
            let parsed_tokens: Result<HashSet<i32>, _> =
                raw.split_whitespace().map(|t| t.parse::<i32>()).collect();
            let valid_keys: HashSet<i32> = mapping.iter().map(|&(k, _)| k).collect();
            let values = match parsed_tokens {
                Ok(set) if !set.is_empty() && set.is_subset(&valid_keys) => set,
                _ => {
                    row.insert(hdr.to_string(), String::new());
                    HashSet::new()
                }
            };
            for &(val, write_hdr) in mapping {
                let bit = if values.contains(&val) { "1" } else { "0" };
                row.insert(write_hdr.to_string(), bit.to_string());
            }
        }

        // process_count_data
        for &hdr in common_data::COUNT_DATA_HEADERS {
            if let Some(v) = row.get(hdr).cloned() {
                let next_v = match v.parse::<i32>() {
                    Ok(iv) => iv.to_string(),
                    Err(_) => String::new(),
                };
                row.insert(hdr.to_string(), next_v);
            }
        }

        // convert_rash_data
        for &(var, write_vars, locations, valid_vals, everywhere) in common_data::RASH_DATA {
            if let Some(raw) = row.get(var).cloned() {
                let parsed: Result<HashSet<i32>, _> =
                    raw.split(' ').map(|t| t.parse::<i32>()).collect();
                if let Ok(mut rash_values) = parsed {
                    let valid_set: HashSet<i32> = valid_vals.iter().copied().collect();
                    let loc_set: HashSet<i32> = locations.iter().copied().collect();
                    if !rash_values.is_subset(&valid_set) {
                        rash_values.clear();
                    }
                    if rash_values.contains(&everywhere) || loc_set.is_subset(&rash_values) {
                        rash_values.clear();
                        rash_values.insert(everywhere);
                    }
                    let mut sorted_vals: Vec<i32> = rash_values.into_iter().collect();
                    sorted_vals.sort_unstable();
                    for (idx, val) in sorted_vals.into_iter().enumerate() {
                        if idx < write_vars.len() {
                            row.insert(write_vars[idx].to_string(), val.to_string());
                        }
                    }
                }
            }
        }

        // convert_weight_data
        for &(uvar, grams_var, kg_var) in common_data::WEIGHT_CONVERSION_DATA {
            let unit = parse_py_int(row.get(uvar).map(|s| s.as_str()));
            let weight = match unit {
                Some(1) => parse_py_float(row.get(grams_var).map(|s| s.as_str())).unwrap_or(0.0),
                Some(2) => {
                    parse_py_float(row.get(kg_var).map(|s| s.as_str())).unwrap_or(0.0) * 1000.0
                }
                _ => 0.0,
            };
            row.insert(uvar.to_string(), "1".to_string());
            let g_str = if weight != 0.0 {
                (weight as i32).to_string()
            } else {
                String::new()
            };
            row.insert(grams_var.to_string(), g_str);
            row.insert(kg_var.to_string(), String::new());
        }

        // convert_free_text
        for &ft_var in common_data::FREE_TEXT_VARS {
            if let Some(raw) = row.get(ft_var).cloned() {
                let cleaned: String = raw
                    .to_lowercase()
                    .chars()
                    .filter(|&c| c.is_ascii_lowercase() || c == ' ')
                    .collect();
                let words: Vec<&str> = cleaned
                    .split(' ')
                    .filter(|w| !w.is_empty())
                    .map(|w| word_subs.get(w).copied().unwrap_or(w))
                    .collect();
                row.insert(ft_var.to_string(), words.join(" "));
            }
        }

        // save_row -> determine module
        let years = safe_int(row.get("gen_5_4a").map(|s| s.as_str()));
        let months = safe_int(row.get("gen_5_4b").map(|s| s.as_str()));
        let days = safe_int(row.get("gen_5_4c").map(|s| s.as_str()));
        let module = safe_int(row.get("gen_5_4d").map(|s| s.as_str()));
        let sid_disp = row
            .get("sid")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "<unknown>".to_string());
        let idx_disp = if index == 0 {
            "<unknown>".to_string()
        } else {
            index.to_string()
        };

        let target_key = if years + months + days == 0 && (module == 8 || module == 9) {
            let msg = format!(
                "SID: {} (row {}) does not have valid age data and is being removed from the analysis.",
                sid_disp, idx_disp
            );
            loggers.warnings.push(msg.clone());
            loggers.valid_age.push(msg);
            "invalid-age"
        } else if years >= 12 || (years == 0 && months == 0 && days == 0 && module == 3) {
            "adult"
        } else if years != 0
            || months != 0
            || days >= 29
            || (years == 0 && months == 0 && days == 0 && module == 2)
        {
            "child"
        } else {
            "neonate"
        };

        matrix_data.get_mut(target_key).unwrap().push(row);
    }

    let keep: HashSet<&str> = headers.iter().map(|s| s.as_str()).collect();
    for rows in matrix_data.values_mut() {
        for row in rows {
            row.retain(|k, _| keep.contains(k.as_str()));
        }
    }

    (headers, matrix_data)
}

/// Run `CommonPrep` on `cleanheaders.csv`, returning `(has_adult, has_child, has_neonate)`.
fn run_common_prep(
    intermediate_dir: &Path,
    short_form: bool,
    loggers: &mut ReportLoggers,
) -> Result<(bool, bool, bool), SmartVaError> {
    let cleanheaders_path = intermediate_dir.join("cleanheaders.csv");
    let (headers, matrix) = read_csv_matrix(&cleanheaders_path)?;
    let (headers, matrix_data) = common_prep_in_memory(headers, matrix, short_form, loggers);

    for (age_key, rows) in &matrix_data {
        if !rows.is_empty() {
            let out_path = intermediate_dir.join(format!("{}-prepped.csv", age_key));
            write_csv_matrix(&out_path, &headers, rows)?;
        }
    }

    Ok((
        !matrix_data["adult"].is_empty(),
        !matrix_data["child"].is_empty(),
        !matrix_data["neonate"].is_empty(),
    ))
}

struct PreSymptomConfig {
    age_group: &'static str,
    default_fill: &'static [(&'static str, &'static str)],
    default_fill_short: &'static [(&'static str, &'static str)],
    words_to_vars: &'static [(&'static str, &'static str)],
    weight_vars: &'static [&'static str],
    date_vars: &'static [&'static str],
    exam_date_vars: &'static [(&'static str, &'static str)],
    generated_vars_data: &'static [(&'static str, &'static str)],
    var_conversion_map: &'static [(&'static str, &'static str)],
    duration_vars: &'static [&'static str],
    duration_vars_short_drop: &'static [&'static str],
    duration_vars_special_case: &'static [(&'static str, i32)],
    duration_days_vars: &'static [&'static str],
    free_text_vars: &'static [&'static str],
    short_form_free_text_conversion: &'static [(&'static str, &'static str)],
    recode_map: &'static [((&'static str, &'static str), &'static [(i32, RecodeTarget)])],
    binary_conversion_map: &'static [(&'static str, BinaryConversionSpec)],
    weight_sd_data: &'static [(&'static str, &'static [(i32, &'static [(i32, f64)])])],
}

fn get_pre_symptom_config(module: &str) -> PreSymptomConfig {
    match module {
        "adult" => PreSymptomConfig {
            age_group: adult_pre_symptom_data::AGE_GROUP,
            default_fill: adult_pre_symptom_data::DEFAULT_FILL,
            default_fill_short: adult_pre_symptom_data::DEFAULT_FILL_SHORT,
            words_to_vars: adult_pre_symptom_data::WORDS_TO_VARS,
            weight_vars: adult_pre_symptom_data::WEIGHT_VARS,
            date_vars: adult_pre_symptom_data::DATE_VARS,
            exam_date_vars: adult_pre_symptom_data::EXAM_DATE_VARS,
            generated_vars_data: adult_pre_symptom_data::GENERATED_VARS_DATA,
            var_conversion_map: adult_pre_symptom_data::VAR_CONVERSION_MAP,
            duration_vars: adult_pre_symptom_data::DURATION_VARS,
            duration_vars_short_drop: adult_pre_symptom_data::DURATION_VARS_SHORT_FORM_DROP_LIST,
            duration_vars_special_case: adult_pre_symptom_data::DURATION_VARS_SPECIAL_CASE,
            duration_days_vars: adult_pre_symptom_data::DURATION_DAYS_VARS,
            free_text_vars: adult_pre_symptom_data::FREE_TEXT_VARS,
            short_form_free_text_conversion:
                adult_pre_symptom_data::SHORT_FORM_FREE_TEXT_CONVERSION,
            recode_map: adult_pre_symptom_data::RECODE_MAP,
            binary_conversion_map: adult_pre_symptom_data::BINARY_CONVERSION_MAP,
            weight_sd_data: adult_pre_symptom_data::WEIGHT_SD_DATA,
        },
        "child" => PreSymptomConfig {
            age_group: child_pre_symptom_data::AGE_GROUP,
            default_fill: child_pre_symptom_data::DEFAULT_FILL,
            default_fill_short: child_pre_symptom_data::DEFAULT_FILL_SHORT,
            words_to_vars: child_pre_symptom_data::WORDS_TO_VARS,
            weight_vars: child_pre_symptom_data::WEIGHT_VARS,
            date_vars: child_pre_symptom_data::DATE_VARS,
            exam_date_vars: child_pre_symptom_data::EXAM_DATE_VARS,
            generated_vars_data: child_pre_symptom_data::GENERATED_VARS_DATA,
            var_conversion_map: child_pre_symptom_data::VAR_CONVERSION_MAP,
            duration_vars: child_pre_symptom_data::DURATION_VARS,
            duration_vars_short_drop: child_pre_symptom_data::DURATION_VARS_SHORT_FORM_DROP_LIST,
            duration_vars_special_case: child_pre_symptom_data::DURATION_VARS_SPECIAL_CASE,
            duration_days_vars: child_pre_symptom_data::DURATION_DAYS_VARS,
            free_text_vars: child_pre_symptom_data::FREE_TEXT_VARS,
            short_form_free_text_conversion:
                child_pre_symptom_data::SHORT_FORM_FREE_TEXT_CONVERSION,
            recode_map: child_pre_symptom_data::RECODE_MAP,
            binary_conversion_map: child_pre_symptom_data::BINARY_CONVERSION_MAP,
            weight_sd_data: child_pre_symptom_data::WEIGHT_SD_DATA,
        },
        _ => PreSymptomConfig {
            age_group: neonate_pre_symptom_data::AGE_GROUP,
            default_fill: neonate_pre_symptom_data::DEFAULT_FILL,
            default_fill_short: neonate_pre_symptom_data::DEFAULT_FILL_SHORT,
            words_to_vars: neonate_pre_symptom_data::WORDS_TO_VARS,
            weight_vars: neonate_pre_symptom_data::WEIGHT_VARS,
            date_vars: neonate_pre_symptom_data::DATE_VARS,
            exam_date_vars: neonate_pre_symptom_data::EXAM_DATE_VARS,
            generated_vars_data: neonate_pre_symptom_data::GENERATED_VARS_DATA,
            var_conversion_map: neonate_pre_symptom_data::VAR_CONVERSION_MAP,
            duration_vars: neonate_pre_symptom_data::DURATION_VARS,
            duration_vars_short_drop: neonate_pre_symptom_data::DURATION_VARS_SHORT_FORM_DROP_LIST,
            duration_vars_special_case: neonate_pre_symptom_data::DURATION_VARS_SPECIAL_CASE,
            duration_days_vars: neonate_pre_symptom_data::DURATION_DAYS_VARS,
            free_text_vars: neonate_pre_symptom_data::FREE_TEXT_VARS,
            short_form_free_text_conversion:
                neonate_pre_symptom_data::SHORT_FORM_FREE_TEXT_CONVERSION,
            recode_map: neonate_pre_symptom_data::RECODE_MAP,
            binary_conversion_map: neonate_pre_symptom_data::BINARY_CONVERSION_MAP,
            weight_sd_data: neonate_pre_symptom_data::WEIGHT_SD_DATA,
        },
    }
}

fn matches_pre_symptom_keep_pattern(module: &str, h: &str) -> bool {
    if h == "sid" || h.starts_with("g5_") {
        return true;
    }
    let bytes = h.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b's' && bytes[1].is_ascii_digit() {
        return true;
    }
    match module {
        "adult" => bytes.len() >= 2 && bytes[0] == b'a' && (bytes[1].is_ascii_digit() || bytes[1] == b'_'),
        "child" => bytes.len() >= 2 && bytes[0] == b'c' && (bytes[1].is_ascii_digit() || bytes[1] == b'_'),
        _ => {
            bytes.len() >= 2
                && (bytes[0] == b'c' || bytes[0] == b'n')
                && (bytes[1].is_ascii_digit() || bytes[1] == b'_')
        }
    }
}

fn matches_symptom_keep_pattern(h: &str) -> bool {
    if matches!(h, "sid" | "age" | "sex" | "cause" | "restricted") || h.starts_with("real") {
        return true;
    }
    let bytes = h.as_bytes();
    bytes.len() >= 2 && bytes[0] == b's' && bytes[1].is_ascii_digit()
}

fn rename_headers_in_place(headers: &mut [String], map: &[(&str, &str)]) {
    for &(old_h, new_h) in map {
        if let Some(pos) = headers.iter().position(|h| h == old_h) {
            headers[pos] = new_h.to_string();
        }
    }
}

fn rename_vars_in_place(row: &mut Row, map: &[(&str, &str)], loggers: &mut ReportLoggers) {
    for &(old_h, new_h) in map {
        if let Some(val) = row.remove(old_h) {
            row.insert(new_h.to_string(), val);
        } else {
            loggers.warnings.push(format!(
                "SID: {} variable '{}' does not exist and cannot be renamed to '{}'.",
                row.get("sid").map(|s| s.as_str()).unwrap_or(""),
                old_h,
                new_h
            ));
        }
    }
}

fn process_binary_vars_in_place(row: &mut Row, map: &[(&str, BinaryConversionSpec)]) {
    for &(data_header, spec) in map {
        let Some(raw) = row.get(data_header).cloned() else {
            continue;
        };
        let trimmed = raw.trim();
        let parsed: Result<Vec<i32>, _> = trimmed.split(' ').map(|t| t.parse::<i32>()).collect();
        let Ok(vals) = parsed else {
            continue;
        };
        for value in vals {
            match spec {
                BinaryConversionSpec::Map(entries) => {
                    if let Some(&(_, target_col)) = entries.iter().find(|&&(k, _)| k == value) {
                        row.insert(target_col.to_string(), "1".to_string());
                    }
                }
                BinaryConversionSpec::List(list) => {
                    let bit = if list.contains(&value) { "1" } else { "0" };
                    row.insert(data_header.to_string(), bit.to_string());
                }
                BinaryConversionSpec::Ldap(expr) => {
                    let bit = eval_ldap(expr, &|k| {
                        row.get(k)
                            .and_then(|s| s.trim().parse::<i32>().ok())
                            .unwrap_or(0) as f64
                    });
                    row.insert(
                        data_header.to_string(),
                        (if bit { 1 } else { 0 }).to_string(),
                    );
                }
            }
        }
    }
}

fn pre_symptom_prep_in_memory(
    module: &str,
    mut headers: Vec<String>,
    mut matrix: Vec<Row>,
    short_form: bool,
    loggers: &mut ReportLoggers,
) -> (Vec<String>, Vec<Row>) {
    let cfg = get_pre_symptom_config(module);
    let default_fill = if short_form {
        cfg.default_fill_short
    } else {
        cfg.default_fill
    };
    let mut duration_vars: Vec<&str> = cfg.duration_vars.to_vec();
    if short_form {
        for &drop_v in cfg.duration_vars_short_drop {
            if let Some(pos) = duration_vars.iter().position(|&x| x == drop_v) {
                duration_vars.remove(pos);
            }
        }
    }

    // Build additional_data in exact order
    let mut additional_map: Vec<(String, String)> = Vec::new();
    let mut put_add = |k: String, v: String| {
        if let Some(pos) = additional_map.iter().position(|(ek, _)| *ek == k) {
            additional_map[pos].1 = v;
        } else {
            additional_map.push((k, v));
        }
    };
    for &k in cfg.duration_vars {
        put_add(k.to_string(), String::new());
    }
    for &k in cfg.duration_days_vars {
        put_add(k.to_string(), String::new());
    }
    for &(k, v) in cfg.generated_vars_data {
        put_add(k.to_string(), v.to_string());
    }
    let mut word_var_vals: Vec<&str> = cfg.words_to_vars.iter().map(|&(_, v)| v).collect();
    word_var_vals.sort_unstable();
    for v in word_var_vals {
        put_add(v.to_string(), "0".to_string());
    }

    let existing_set: HashSet<String> = headers.iter().cloned().collect();
    let mut add_headers = Vec::new();
    let mut add_values = Vec::new();
    for (k, v) in additional_map {
        if !existing_set.contains(&k) {
            add_headers.push(k);
            add_values.push(v);
        }
    }
    headers.extend(add_headers.iter().cloned());
    rename_headers_in_place(&mut headers, cfg.var_conversion_map);

    let drop_set: HashSet<String> = duration_vars
        .iter()
        .flat_map(|&v| [format!("{}a", v), format!("{}b", v)])
        .collect();

    headers.retain(|h| matches_pre_symptom_keep_pattern(module, h) && !drop_set.contains(h));
    headers.sort_by(|a, b| {
        let key_a = (
            a != "sid",
            a.as_bytes().get(1).is_some_and(|c| c.is_ascii_digit()),
            !a.starts_with('g'),
            a.as_str(),
        );
        let key_b = (
            b != "sid",
            b.as_bytes().get(1).is_some_and(|c| c.is_ascii_digit()),
            !b.starts_with('g'),
            b.as_str(),
        );
        key_a.cmp(&key_b)
    });

    let words_lookup: HashMap<&str, &str> = cfg.words_to_vars.iter().copied().collect();

    for row in &mut matrix {
        for &(old_h, _) in cfg.var_conversion_map {
            if existing_set.contains(old_h) && !row.contains_key(old_h) {
                row.insert(old_h.to_string(), String::new());
            }
        }
        for (k, v) in add_headers.iter().zip(add_values.iter()) {
            if !row.contains_key(k) {
                row.insert(k.clone(), v.clone());
            }
        }
        rename_vars_in_place(row, cfg.var_conversion_map, loggers);
        for h in &headers {
            if !row.contains_key(h) {
                row.insert(h.clone(), String::new());
            }
        }
        for h in &drop_set {
            if !row.contains_key(h) {
                row.insert(h.clone(), String::new());
            }
        }
        for &((read_h, _), _) in cfg.recode_map {
            if !row.contains_key(read_h) {
                row.insert(read_h.to_string(), String::new());
            }
        }

        // verify_answers_for_row
        for &(variable, ranges) in answer_ranges::RANGE_LIST {
            if let Some(value) = row.get(variable) {
                for answer in value.split_whitespace() {
                    if let Ok(iv) = answer.parse::<i32>() {
                        let in_range = ranges.iter().any(|&(lo, hi)| iv >= lo && iv <= hi);
                        if !in_range {
                            loggers.warnings.push(format!(
                                "SID: {} variable '{}' has an illegal value {}. Please see code book for legal values.",
                                row.get("sid").map(|s| s.as_str()).unwrap_or(""),
                                variable,
                                value
                            ));
                        }
                    }
                }
            }
        }

        // fix_agedays
        if module == "child" || module == "neonate" {
            if row.contains_key("c1_25b") {
                let v = parse_py_int(row.get("c1_25b").map(|s| s.as_str()));
                let c1_25a = if v.is_none() { "0" } else { "4" };
                row.insert("c1_25a".to_string(), c1_25a.to_string());
            }
        }

        // calculate_age_at_death_value
        if module == "neonate" {
            if row.contains_key("c1_25b") {
                let v = parse_py_int(row.get("c1_25b").map(|s| s.as_str())).unwrap_or(0);
                if (1..=28).contains(&v) {
                    row.insert("c1_26".to_string(), "1".to_string());
                }
            }
        } else if module == "child" {
            row.insert("c1_26".to_string(), "2".to_string());
        }

        // recode_answers
        for &((read_h, write_h), data_map) in cfg.recode_map {
            if let Some(raw) = row.get(read_h).cloned() {
                if let Ok(val) = raw.trim().parse::<i32>() {
                    if let Some(&(_, target)) = data_map.iter().find(|&&(k, _)| k == val) {
                        match target {
                            RecodeTarget::Column(col) => {
                                let v = row.get(col).cloned().unwrap_or_default();
                                row.insert(write_h.to_string(), v);
                            }
                            RecodeTarget::Literal(lit) => {
                                row.insert(write_h.to_string(), lit.to_string());
                            }
                        }
                    }
                }
            } else {
                loggers.warnings.push(format!(
                    "SID: {} variable '{}' does not exist.",
                    row.get("sid").map(|s| s.as_str()).unwrap_or(""),
                    read_h
                ));
            }
        }

        // process_binary_vars
        process_binary_vars_in_place(row, cfg.binary_conversion_map);

        // calculate_duration_vars
        for &var in &duration_vars {
            let code_var = format!("{}a", var);
            let length_var = format!("{}b", var);
            let (Some(code_raw), Some(length_raw)) =
                (row.get(&code_var).cloned(), row.get(&length_var).cloned())
            else {
                continue;
            };
            let code_val = parse_py_int(Some(&code_raw)).unwrap_or(0);
            let length_val = parse_py_int(Some(&length_raw)).unwrap_or(0);
            if let Some(&(_, spec_val)) = cfg
                .duration_vars_special_case
                .iter()
                .find(|&&(k, _)| k == var)
            {
                if length_raw.is_empty() {
                    row.insert(var.to_string(), spec_val.to_string());
                    continue;
                }
            }
            let factor = match code_val {
                1 => Some(365.0),
                2 => Some(30.0),
                3 => Some(7.0),
                4 => Some(1.0),
                5 => Some(1.0 / 24.0),
                6 => Some(1.0 / 1440.0),
                _ => None,
            };
            let formatted = match factor {
                Some(f) => py_float_str(f * (length_val as f64)),
                None => "0".to_string(),
            };
            row.insert(var.to_string(), formatted);
        }

        // validate_days_vars
        for &var in cfg.duration_days_vars {
            let code_var = format!("{}a", var);
            let length_var = format!("{}b", var);
            if let (Some(cv), Some(lv)) = (
                parse_py_int(row.get(&code_var).map(|s| s.as_str())),
                parse_py_int(row.get(&length_var).map(|s| s.as_str())),
            ) {
                let res = if cv == 2 { lv } else { 0 };
                row.insert(var.to_string(), res.to_string());
            }
        }

        // validate_weight_vars
        for &var in cfg.weight_vars {
            if let Some(raw) = row.get(var).cloned() {
                let next_v = match parse_py_int(Some(&raw)) {
                    Some(iv) if iv != 0 && iv != 9999 => iv.to_string(),
                    _ => String::new(),
                };
                row.insert(var.to_string(), next_v);
            }
        }

        // validate_date_vars
        for &var in cfg.date_vars {
            for (suffix, invalids, def_val) in [
                ("d", &["", "99"][..], "1"),
                ("m", &["", "99"][..], "1"),
                ("y", &["", "999", "9999"][..], "0"),
            ] {
                let var_name = format!("{}{}", var, suffix);
                if let Some(cur) = row.get(&var_name) {
                    if invalids.contains(&cur.as_str()) {
                        row.insert(var_name, def_val.to_string());
                    }
                }
            }
        }

        // process_age_vars
        if row.contains_key("g5_04a") && row.contains_key("g5_04b") && row.contains_key("g5_04c") {
            let parse_age_part = |s: &str, inv: &[f64]| -> f64 {
                match parse_py_float(Some(s)) {
                    Some(v) if !inv.contains(&v) => v,
                    _ => 0.0,
                }
            };
            let years = parse_age_part(
                row.get("g5_04a").map(|s| s.as_str()).unwrap_or(""),
                &[999.0, 9999.0],
            );
            let months = parse_age_part(
                row.get("g5_04b").map(|s| s.as_str()).unwrap_or(""),
                &[99.0],
            );
            let days = parse_age_part(
                row.get("g5_04c").map(|s| s.as_str()).unwrap_or(""),
                &[99.0],
            );
            row.insert(
                "g5_04a".to_string(),
                py_float_str(years + (months / 12.0) + (days / 365.0)),
            );
            row.insert(
                "g5_04b".to_string(),
                py_float_str((12.0 * years) + months + (days / 30.0)),
            );
            row.insert(
                "g5_04c".to_string(),
                py_float_str((365.0 * years) + (30.0 * months) + days),
            );
        }

        // convert_free_text_vars
        for &ft_hdr in cfg.free_text_vars {
            if let Some(raw) = row.get(ft_hdr).cloned() {
                if !raw.is_empty() {
                    for word in raw.split(' ') {
                        let stemmed = stem_porter2(word);
                        if let Some(&target_var) = words_lookup.get(stemmed.as_str()) {
                            row.insert(target_var.to_string(), "1".to_string());
                        }
                    }
                }
            }
        }

        // short_form free text conversion
        if short_form {
            let mut word_list = Vec::new();
            for &(k, v) in cfg.short_form_free_text_conversion {
                if parse_py_int(row.get(k).map(|s| s.as_str())).unwrap_or(0) == 1 {
                    word_list.push(v);
                }
            }
            for word in word_list {
                let stemmed = stem_porter2(word);
                if let Some(&target_var) = words_lookup.get(stemmed.as_str()) {
                    row.insert(target_var.to_string(), "1".to_string());
                }
            }
        }

        // fix_rash_length
        if let Some(c4_33a) = row.get("c4_33a") {
            if let Ok(iv) = c4_33a.trim().parse::<i32>() {
                if iv != 4 {
                    row.insert("c4_33b".to_string(), "0".to_string());
                }
            }
        }

        // fix_rash_location
        if module == "child" {
            if row.contains_key("c4_31_1") && row.contains_key("c4_32") {
                for var in ["c4_31_1", "c4_32"] {
                    let has_one = row
                        .get(var)
                        .map(|s| s.split_whitespace().any(|w| w == "1"))
                        .unwrap_or(false);
                    row.insert(
                        var.to_string(),
                        (if has_one { "1" } else { "0" }).to_string(),
                    );
                }
            }
        }

        // process_weight_sd_vars
        process_weight_sd_vars(row, cfg.exam_date_vars, cfg.weight_sd_data);

        // fill_missing_data
        for &(var, def_val) in default_fill {
            if let Some(cur) = row.get_mut(var) {
                if cur.is_empty() {
                    *cur = def_val.to_string();
                }
            }
        }
    }

    project_matrix_to_headers(&headers, &mut matrix);
    (headers, matrix)
}

fn read_csv_matrix_sparse(path: &Path) -> Result<(Vec<String>, Vec<Row>), SmartVaError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)
        .map_err(|e| SmartVaError::IoError(format!("Failed to read CSV {}: {}", path.display(), e)))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut matrix = Vec::new();
    for result in rdr.records() {
        let record = result.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        let mut row = Row::new();
        for (i, h) in headers.iter().enumerate() {
            let val = record.get(i).unwrap_or("");
            if !val.is_empty() {
                row.insert(h.clone(), val.to_string());
            }
        }
        matrix.push(row);
    }
    Ok((headers, matrix))
}

fn run_pre_symptom_prep(
    intermediate_dir: &Path,
    module: &str,
    short_form: bool,
    loggers: &mut ReportLoggers,
) -> Result<(), SmartVaError> {
    let cfg = get_pre_symptom_config(module);
    let in_path = intermediate_dir.join(format!("{}-prepped.csv", cfg.age_group));
    let (headers, matrix) = read_csv_matrix_sparse(&in_path)?;
    let (headers, matrix) =
        pre_symptom_prep_in_memory(module, headers, matrix, short_form, loggers);
    let out_path = intermediate_dir.join(format!("{}-presymptom.csv", cfg.age_group));
    write_csv_matrix(&out_path, &headers, &matrix)
}

fn parse_row_date(row: &Row, prefix: &str) -> Option<chrono::NaiveDate> {
    let y = parse_py_int(row.get(&format!("{}y", prefix)).map(|s| s.as_str()))?;
    let m = parse_py_int(row.get(&format!("{}m", prefix)).map(|s| s.as_str()))?;
    let d = parse_py_int(row.get(&format!("{}d", prefix)).map(|s| s.as_str()))?;
    if y < 1 || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(y, m as u32, d as u32)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    chrono::NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(30)
}

fn relativedelta_months(latest: chrono::NaiveDate, dob: chrono::NaiveDate) -> i32 {
    let mut months = (latest.year() - dob.year()) * 12 + (latest.month() as i32 - dob.month() as i32);
    let clamped_dob_day = dob.day().min(days_in_month(latest.year(), latest.month()));
    if latest.day() < clamped_dob_day {
        months -= 1;
    }
    months.abs()
}

fn process_weight_sd_vars(
    row: &mut Row,
    exam_date_vars: &[(&str, &str)],
    weight_sd_data: &[(&str, &[(i32, &[(i32, f64)])])],
) {
    if exam_date_vars.is_empty() {
        return;
    }
    let Some(dob) = parse_row_date(row, "g5_01") else {
        return;
    };
    let mut exam_data: Vec<(chrono::NaiveDate, f64)> = Vec::new();
    for &(date_var, weight_var) in exam_date_vars {
        let Some(exam_date) = parse_row_date(row, date_var) else {
            continue;
        };
        let Some(exam_weight) =
            parse_py_float(row.get(&format!("{}b", weight_var)).map(|s| s.as_str()))
        else {
            continue;
        };
        exam_data.push((exam_date, exam_weight));
    }
    if exam_data.is_empty() {
        return;
    }
    exam_data.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.total_cmp(&a.1)));
    let (latest_exam, latest_weight) = exam_data[0];
    if latest_exam > dob {
        let age_at_exam_months = relativedelta_months(latest_exam, dob);
        if age_at_exam_months <= 60 {
            let sex = safe_int(row.get("g5_02").map(|s| s.as_str()));
            let weight_kg = latest_weight / 1000.0;
            for &(sd_var, sex_table) in weight_sd_data {
                let threshold = sex_table
                    .iter()
                    .find(|&&(s, _)| s == sex)
                    .and_then(|&(_, m_table)| {
                        m_table
                            .iter()
                            .find(|&&(m, _)| m == age_at_exam_months)
                            .map(|&(_, t)| t)
                    })
                    .unwrap_or(0.0);
                let val = if weight_kg < threshold { "1" } else { "0" };
                row.insert(sd_var.to_string(), val.to_string());
            }
        }
    }
}

fn rules_prep_in_memory(module: &str, mut headers: Vec<String>, mut matrix: Vec<Row>) -> (Vec<String>, Vec<Row>) {
    headers.push("cause".to_string());
    for row in &mut matrix {
        if !row.contains_key("cause") {
            row.insert("cause".to_string(), String::new());
        }
        if let Some(cause_id) = evaluate_module_rules(module, row) {
            row.insert("cause".to_string(), cause_id.to_string());
        }
    }
    project_matrix_to_headers(&headers, &mut matrix);
    (headers, matrix)
}

fn run_rules_prep(intermediate_dir: &Path, module: &str) -> Result<(), SmartVaError> {
    let in_path = intermediate_dir.join(format!("{}-presymptom.csv", module));
    let (headers, matrix) = read_csv_matrix(&in_path)?;
    let (headers, matrix) = rules_prep_in_memory(module, headers, matrix);
    let out_path = intermediate_dir.join(format!("{}-logic-rules.csv", module));
    write_csv_matrix(&out_path, &headers, &matrix)
}

struct SymptomConfig {
    age_group: &'static str,
    generated_vars_data: &'static [(&'static str, &'static str)],
    var_conversion_map: &'static [(&'static str, &'static str)],
    copy_vars: &'static [(&'static str, &'static str)],
    age_quartile_binary_vars:
        &'static [(&'static str, &'static [(Option<f64>, &'static str, i32)])],
    duration_cutoff_data: &'static [(&'static str, f64)],
    injury_vars: &'static [((&'static str, f64), &'static [&'static str])],
    binary_conversion_map: &'static [(&'static str, BinaryConversionSpec)],
    binary_vars: &'static [&'static str],
    drop_list: &'static [&'static str],
    censored_map: &'static [(i32, &'static [&'static str])],
    required_map: &'static [(i32, &'static [&'static str])],
}

fn get_symptom_config(module: &str) -> SymptomConfig {
    match module {
        "adult" => SymptomConfig {
            age_group: adult_symptom_data::AGE_GROUP,
            generated_vars_data: adult_symptom_data::GENERATED_VARS_DATA,
            var_conversion_map: adult_symptom_data::VAR_CONVERSION_MAP,
            copy_vars: adult_symptom_data::COPY_VARS,
            age_quartile_binary_vars: adult_symptom_data::AGE_QUARTILE_BINARY_VARS,
            duration_cutoff_data: adult_symptom_data::DURATION_CUTOFF_DATA,
            injury_vars: adult_symptom_data::INJURY_VARS,
            binary_conversion_map: adult_symptom_data::BINARY_CONVERSION_MAP,
            binary_vars: adult_symptom_data::BINARY_VARS,
            drop_list: adult_symptom_data::DROP_LIST,
            censored_map: adult_symptom_data::CENSORED_MAP,
            required_map: adult_symptom_data::REQUIRED_MAP,
        },
        "child" => SymptomConfig {
            age_group: child_symptom_data::AGE_GROUP,
            generated_vars_data: child_symptom_data::GENERATED_VARS_DATA,
            var_conversion_map: child_symptom_data::VAR_CONVERSION_MAP,
            copy_vars: child_symptom_data::COPY_VARS,
            age_quartile_binary_vars: child_symptom_data::AGE_QUARTILE_BINARY_VARS,
            duration_cutoff_data: child_symptom_data::DURATION_CUTOFF_DATA,
            injury_vars: child_symptom_data::INJURY_VARS,
            binary_conversion_map: child_symptom_data::BINARY_CONVERSION_MAP,
            binary_vars: child_symptom_data::BINARY_VARS,
            drop_list: child_symptom_data::DROP_LIST,
            censored_map: child_symptom_data::CENSORED_MAP,
            required_map: child_symptom_data::REQUIRED_MAP,
        },
        _ => SymptomConfig {
            age_group: neonate_symptom_data::AGE_GROUP,
            generated_vars_data: neonate_symptom_data::GENERATED_VARS_DATA,
            var_conversion_map: neonate_symptom_data::VAR_CONVERSION_MAP,
            copy_vars: neonate_symptom_data::COPY_VARS,
            age_quartile_binary_vars: neonate_symptom_data::AGE_QUARTILE_BINARY_VARS,
            duration_cutoff_data: neonate_symptom_data::DURATION_CUTOFF_DATA,
            injury_vars: neonate_symptom_data::INJURY_VARS,
            binary_conversion_map: neonate_symptom_data::BINARY_CONVERSION_MAP,
            binary_vars: neonate_symptom_data::BINARY_VARS,
            drop_list: neonate_symptom_data::DROP_LIST,
            censored_map: neonate_symptom_data::CENSORED_MAP,
            required_map: neonate_symptom_data::REQUIRED_MAP,
        },
    }
}

fn symptom_prep_in_memory(
    module: &str,
    mut headers: Vec<String>,
    mut matrix: Vec<Row>,
    loggers: &mut ReportLoggers,
) -> (Vec<String>, Vec<Row>) {
    let cfg = get_symptom_config(module);

    let existing_set: HashSet<String> = headers.iter().cloned().collect();
    let mut add_headers = Vec::new();
    let mut add_values = Vec::new();
    for &(k, v) in cfg.generated_vars_data {
        if !existing_set.contains(k) {
            add_headers.push(k.to_string());
            add_values.push(v.to_string());
        }
    }
    headers.extend(add_headers.iter().cloned());
    rename_headers_in_place(&mut headers, cfg.var_conversion_map);

    let drop_set: HashSet<&str> = cfg.drop_list.iter().copied().collect();
    headers.retain(|h| matches_symptom_keep_pattern(h) && !drop_set.contains(h.as_str()));
    headers.sort_by(|a, b| {
        let key_a = (
            a != "sid",
            a.as_bytes().get(1).is_some_and(|c| c.is_ascii_digit()),
            a.as_str(),
        );
        let key_b = (
            b != "sid",
            b.as_bytes().get(1).is_some_and(|c| c.is_ascii_digit()),
            b.as_str(),
        );
        key_a.cmp(&key_b)
    });

    for row in &mut matrix {
        for (k, v) in add_headers.iter().zip(add_values.iter()) {
            if !row.contains_key(k) {
                row.insert(k.clone(), v.clone());
            }
        }
        rename_vars_in_place(row, cfg.var_conversion_map, loggers);

        // copy_variables
        for &(rh, wh) in cfg.copy_vars {
            if let Some(val) = row.get(rh).cloned() {
                row.insert(wh.to_string(), val);
            }
        }

        // process_progressive_value_data
        for &(read_h, entries) in cfg.age_quartile_binary_vars {
            if !row.contains_key(read_h) {
                continue;
            }
            let Some(cur_val) = parse_py_float(row.get(read_h).map(|s| s.as_str())) else {
                continue;
            };
            for &(thresh_opt, write_h, write_val) in entries {
                if thresh_opt.is_none_or(|t| cur_val > t) {
                    row.insert(write_h.to_string(), write_val.to_string());
                    break;
                }
            }
        }

        // process_cutoff_data
        for &(read_h, cutoff) in cfg.duration_cutoff_data {
            if let Some(raw) = row.get(read_h).cloned() {
                let bit = match parse_py_float(Some(&raw)) {
                    Some(v) if v >= cutoff => "1",
                    _ => "0",
                };
                row.insert(read_h.to_string(), bit.to_string());
            }
        }

        // process_injury_data
        for &((read_h, cutoff), inj_list) in cfg.injury_vars {
            let period = safe_float(row.get(read_h).map(|s| s.as_str()));
            if period > cutoff {
                for &inj in inj_list {
                    row.insert(inj.to_string(), "0".to_string());
                }
            }
        }

        // process_binary_vars
        process_binary_vars_in_place(row, cfg.binary_conversion_map);

        // post_process_binary_variables
        for &bin_h in cfg.binary_vars {
            if let Some(raw) = row.get(bin_h).cloned() {
                let iv = raw.trim().parse::<i32>().unwrap_or(0);
                let bit = if iv == 1 { "1" } else { "0" };
                row.insert(bin_h.to_string(), bit.to_string());
            }
        }

        // censor_causes & require_symptoms
        let mut restricted = BTreeSet::new();
        for &(cause, symps) in cfg.censored_map {
            if symps
                .iter()
                .any(|&s| safe_int(row.get(s).map(|v| v.as_str())) != 0)
            {
                restricted.insert(cause);
            }
        }
        for &(cause, symps) in cfg.required_map {
            let all_endorsed = symps.iter().all(|&s| {
                row.get(s)
                    .and_then(|v| v.trim().parse::<i32>().ok())
                    .unwrap_or(0)
                    != 0
            });
            if !all_endorsed {
                restricted.insert(cause);
            }
        }
        let restricted_str = restricted
            .into_iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        row.insert("restricted".to_string(), restricted_str);
    }

    project_matrix_to_headers(&headers, &mut matrix);
    (headers, matrix)
}

fn run_symptom_prep(
    intermediate_dir: &Path,
    module: &str,
    loggers: &mut ReportLoggers,
) -> Result<(), SmartVaError> {
    let cfg = get_symptom_config(module);
    let in_path = intermediate_dir.join(format!("{}-logic-rules.csv", cfg.age_group));
    let (headers, matrix) = read_csv_matrix(&in_path)?;
    let (headers, matrix) = symptom_prep_in_memory(module, headers, matrix, loggers);
    let out_path = intermediate_dir.join(format!("{}-symptom.csv", cfg.age_group));
    write_csv_matrix(&out_path, &headers, &matrix)
}

struct TariffConfig {
    age_group: &'static str,
    cutoff_pos: f64,
    uniform_list_pos: f64,
    min_cause_score_default: f64,
    min_cause_score_overrides: &'static [(i32, f64)],
    free_text: &'static [&'static str],
    hce_drop_list: &'static [&'static str],
    short_form_drop_list: &'static [&'static str],
    spurious_associations: &'static [(i32, &'static [&'static str])],
    causes: &'static [(i32, &'static str)],
    causes46: &'static [(i32, &'static str)],
    cause_reduction: &'static [(i32, i32)],
    cause_conditions: &'static [(&'static str, &'static [i32])],
    hiv_causes: &'static [i32],
    malaria_causes: &'static [i32],
    female_causes: &'static [i32],
    maternal_causes: &'static [i32],
    male_causes: &'static [i32],
    rule_key_symptoms: &'static [(i32, &'static str)],
    symptom_descriptions: &'static [(&'static str, &'static str)],
    frequencies: &'static [(&'static str, i32)],
}

fn get_tariff_config(module: &str) -> TariffConfig {
    match module {
        "adult" => TariffConfig {
            age_group: adult_tariff_data::AGE_GROUP,
            cutoff_pos: adult_tariff_data::CUTOFF_POS,
            uniform_list_pos: adult_tariff_data::UNIFORM_LIST_POS,
            min_cause_score_default: adult_tariff_data::MIN_CAUSE_SCORE_DEFAULT,
            min_cause_score_overrides: adult_tariff_data::MIN_CAUSE_SCORE_OVERRIDES,
            free_text: adult_tariff_data::FREE_TEXT,
            hce_drop_list: adult_tariff_data::HCE_DROP_LIST,
            short_form_drop_list: adult_tariff_data::SHORT_FORM_DROP_LIST,
            spurious_associations: adult_tariff_data::SPURIOUS_ASSOCIATIONS,
            causes: adult_tariff_data::CAUSES,
            causes46: adult_tariff_data::CAUSES46,
            cause_reduction: adult_tariff_data::CAUSE_REDUCTION,
            cause_conditions: adult_tariff_data::CAUSE_CONDITIONS,
            hiv_causes: adult_tariff_data::HIV_CAUSES,
            malaria_causes: adult_tariff_data::MALARIA_CAUSES,
            female_causes: adult_tariff_data::FEMALE_CAUSES,
            maternal_causes: adult_tariff_data::MATERNAL_CAUSES,
            male_causes: adult_tariff_data::MALE_CAUSES,
            rule_key_symptoms: adult_tariff_data::RULE_KEY_SYMPTOMS,
            symptom_descriptions: adult_tariff_data::SYMPTOM_DESCRIPTIONS,
            frequencies: adult_tariff_data::FREQUENCIES,
        },
        "child" => TariffConfig {
            age_group: child_tariff_data::AGE_GROUP,
            cutoff_pos: child_tariff_data::CUTOFF_POS,
            uniform_list_pos: child_tariff_data::UNIFORM_LIST_POS,
            min_cause_score_default: child_tariff_data::MIN_CAUSE_SCORE_DEFAULT,
            min_cause_score_overrides: child_tariff_data::MIN_CAUSE_SCORE_OVERRIDES,
            free_text: child_tariff_data::FREE_TEXT,
            hce_drop_list: child_tariff_data::HCE_DROP_LIST,
            short_form_drop_list: child_tariff_data::SHORT_FORM_DROP_LIST,
            spurious_associations: child_tariff_data::SPURIOUS_ASSOCIATIONS,
            causes: child_tariff_data::CAUSES,
            causes46: child_tariff_data::CAUSES46,
            cause_reduction: child_tariff_data::CAUSE_REDUCTION,
            cause_conditions: child_tariff_data::CAUSE_CONDITIONS,
            hiv_causes: child_tariff_data::HIV_CAUSES,
            malaria_causes: child_tariff_data::MALARIA_CAUSES,
            female_causes: child_tariff_data::FEMALE_CAUSES,
            maternal_causes: child_tariff_data::MATERNAL_CAUSES,
            male_causes: child_tariff_data::MALE_CAUSES,
            rule_key_symptoms: child_tariff_data::RULE_KEY_SYMPTOMS,
            symptom_descriptions: child_tariff_data::SYMPTOM_DESCRIPTIONS,
            frequencies: child_tariff_data::FREQUENCIES,
        },
        _ => TariffConfig {
            age_group: neonate_tariff_data::AGE_GROUP,
            cutoff_pos: neonate_tariff_data::CUTOFF_POS,
            uniform_list_pos: neonate_tariff_data::UNIFORM_LIST_POS,
            min_cause_score_default: neonate_tariff_data::MIN_CAUSE_SCORE_DEFAULT,
            min_cause_score_overrides: neonate_tariff_data::MIN_CAUSE_SCORE_OVERRIDES,
            free_text: neonate_tariff_data::FREE_TEXT,
            hce_drop_list: neonate_tariff_data::HCE_DROP_LIST,
            short_form_drop_list: neonate_tariff_data::SHORT_FORM_DROP_LIST,
            spurious_associations: neonate_tariff_data::SPURIOUS_ASSOCIATIONS,
            causes: neonate_tariff_data::CAUSES,
            causes46: neonate_tariff_data::CAUSES46,
            cause_reduction: neonate_tariff_data::CAUSE_REDUCTION,
            cause_conditions: neonate_tariff_data::CAUSE_CONDITIONS,
            hiv_causes: neonate_tariff_data::HIV_CAUSES,
            malaria_causes: neonate_tariff_data::MALARIA_CAUSES,
            female_causes: neonate_tariff_data::FEMALE_CAUSES,
            maternal_causes: neonate_tariff_data::MATERNAL_CAUSES,
            male_causes: neonate_tariff_data::MALE_CAUSES,
            rule_key_symptoms: neonate_tariff_data::RULE_KEY_SYMPTOMS,
            symptom_descriptions: neonate_tariff_data::SYMPTOM_DESCRIPTIONS,
            frequencies: neonate_tariff_data::FREQUENCIES,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TrainingCacheKey {
    module: String,
    hce: bool,
    free_text: bool,
    short_form: bool,
}

#[derive(Debug, Clone)]
struct TrainedModuleModel {
    cause_list: Vec<i32>,
    tariffs: BTreeMap<i32, Vec<(String, f64)>>,
    uniform_len: usize,
    uniform_scores: BTreeMap<i32, Vec<f64>>,
    cutoffs: BTreeMap<i32, usize>,
    likelihoods: BTreeMap<i32, Vec<f64>>,
}

static TRAINING_CACHE: OnceLock<Mutex<HashMap<TrainingCacheKey, Arc<TrainedModuleModel>>>> =
    OnceLock::new();

fn get_or_build_trained_model(
    module: &str,
    hce: bool,
    free_text: bool,
    short_form: bool,
) -> Result<Arc<TrainedModuleModel>, SmartVaError> {
    let cache = TRAINING_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = TrainingCacheKey {
        module: module.to_string(),
        hce,
        free_text,
        short_form,
    };
    if let Some(cached) = cache.lock().unwrap().get(&key).cloned() {
        return Ok(cached);
    }

    let cfg = get_tariff_config(module);
    let tariffs_csv = data::embedded_tariffs_csv(cfg.age_group).ok_or_else(|| {
        SmartVaError::CsvError(format!("Missing embedded tariffs CSV for {}", cfg.age_group))
    })?;
    let validated_csv = data::embedded_validated_csv(cfg.age_group).ok_or_else(|| {
        SmartVaError::CsvError(format!(
            "Missing embedded validated CSV for {}",
            cfg.age_group
        ))
    })?;

    let mut drop_headers: HashSet<&str> = HashSet::from(["xs_name"]);
    if !hce {
        drop_headers.extend(cfg.hce_drop_list.iter().copied());
    }
    if !free_text {
        drop_headers.extend(cfg.free_text.iter().copied());
    }
    if short_form {
        drop_headers.extend(cfg.short_form_drop_list.iter().copied());
    }

    let spurious_map: HashMap<i32, HashSet<&str>> = cfg
        .spurious_associations
        .iter()
        .map(|&(c, list)| (c, list.iter().copied().collect()))
        .collect();

    // Load embedded tariffs-{module}.csv preserving exact column order for stable top-40 sort
    let mut t_rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(tariffs_csv.as_bytes());
    let t_headers: Vec<String> = t_rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let xs_idx = t_headers
        .iter()
        .position(|h| h == "xs_name")
        .ok_or_else(|| SmartVaError::CsvError("Missing xs_name in tariffs CSV".to_string()))?;

    let mut tariffs: BTreeMap<i32, Vec<(String, f64)>> = BTreeMap::new();
    for rec_res in t_rdr.records() {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        let xs_name = rec.get(xs_idx).unwrap_or("");
        let cause_num: i32 = xs_name
            .trim_start_matches("cause")
            .parse()
            .map_err(|e| SmartVaError::CsvError(format!("Invalid cause {}: {}", xs_name, e)))?;
        let spurious = spurious_map.get(&cause_num);
        let mut items: Vec<(String, f64)> = Vec::new();
        for (i, h) in t_headers.iter().enumerate() {
            if drop_headers.contains(h.as_str()) {
                continue;
            }
            if spurious.is_some_and(|sp| sp.contains(h.as_str())) {
                continue;
            }
            let raw_v = rec.get(i).unwrap_or("");
            let fv = safe_float(Some(raw_v));
            if fv != 0.0 {
                items.push((h.clone(), fv));
            }
        }
        items.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        items.truncate(40);
        let rounded: Vec<(String, f64)> = items
            .into_iter()
            .map(|(k, v)| (k, (v / 0.5).round_ties_even() * 0.5))
            .collect();
        tariffs.insert(cause_num, rounded);
    }

    let cause_list: Vec<i32> = tariffs.keys().copied().collect();
    let freq_map: HashMap<&str, usize> = cfg
        .frequencies
        .iter()
        .map(|&(sid, f)| (sid, f as usize))
        .collect();

    // Read embedded validated-{module}.csv and score using integer symptom IDs for speed
    let mut v_rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(validated_csv.as_bytes());
    let v_headers: Vec<String> = v_rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let v_header_idx: HashMap<&str, usize> = v_headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.as_str(), i))
        .collect();
    let sid_idx = *v_header_idx
        .get("sid")
        .ok_or_else(|| SmartVaError::CsvError("Missing sid in validated CSV".to_string()))?;
    let va46_idx = *v_header_idx
        .get("va46")
        .ok_or_else(|| SmartVaError::CsvError("Missing va46 in validated CSV".to_string()))?;

    let indexed_tariffs: Vec<(i32, Vec<(usize, f64)>)> = cause_list
        .iter()
        .map(|&c| {
            let pairs = tariffs[&c]
                .iter()
                .filter_map(|(sym, w)| v_header_idx.get(sym.as_str()).map(|&idx| (idx, *w)))
                .collect();
            (c, pairs)
        })
        .collect();

    struct ValidatedVa {
        sid: String,
        cause: i32,
        freq: usize,
        scores: Vec<f64>,
    }

    let mut validated_vas: Vec<ValidatedVa> = Vec::new();
    let mut n_uniform: usize = 0;

    for rec_res in v_rdr.records() {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        let sid = rec.get(sid_idx).unwrap_or("");
        let freq = freq_map.get(sid).copied().unwrap_or(0);
        if freq == 0 {
            continue;
        }
        let cause = rec
            .get(va46_idx)
            .unwrap_or("0")
            .trim()
            .parse::<i32>()
            .unwrap_or(0);
        let mut endorsed = vec![false; v_headers.len()];
        for (i, val) in rec.iter().enumerate() {
            if safe_float(Some(val)) != 0.0 {
                endorsed[i] = true;
            }
        }
        let mut scores = Vec::with_capacity(indexed_tariffs.len());
        for (_, pairs) in &indexed_tariffs {
            let mut sum = 0.0;
            for &(s_idx, w) in pairs {
                if endorsed[s_idx] {
                    sum += w;
                }
            }
            scores.push(sum);
        }
        n_uniform += freq;
        validated_vas.push(ValidatedVa {
            sid: sid.to_string(),
            cause,
            freq,
            scores,
        });
    }

    // Sort unique validated VAs once by sid ascending so tie-breaking on (-score, sid)
    // is reduced to comparing integer index `ia.cmp(&ib)`.
    validated_vas.sort_by(|a, b| a.sid.cmp(&b.sid));
    let base_indices: Vec<usize> = (0..validated_vas.len()).collect();

    let overall_cutoff = (n_uniform as f64) * cfg.cutoff_pos;
    let thresholds = [0.25_f64, 0.5_f64, 0.75_f64];

    let mut uniform_scores = BTreeMap::new();
    let mut cutoffs = BTreeMap::new();
    let mut likelihoods = BTreeMap::new();

    for (c_pos, &cause) in cause_list.iter().enumerate() {
        let mut sorted_unique = base_indices.clone();
        sorted_unique.sort_by(|&ia, &ib| {
            let neg_a = -validated_vas[ia].scores[c_pos];
            let neg_b = -validated_vas[ib].scores[c_pos];
            neg_a.total_cmp(&neg_b).then_with(|| ia.cmp(&ib))
        });

        let mut low_to_high: Vec<f64> = Vec::with_capacity(n_uniform);
        for &va_i in sorted_unique.iter().rev() {
            let va = &validated_vas[va_i];
            low_to_high.extend(std::iter::repeat_n(va.scores[c_pos], va.freq));
        }
        uniform_scores.insert(cause, low_to_high);

        let mut ranks_for_cause: Vec<usize> = Vec::new();
        let mut rank_offset = 0usize;
        for &va_i in &sorted_unique {
            let va = &validated_vas[va_i];
            if va.cause == cause {
                ranks_for_cause.extend((rank_offset + 1)..=(rank_offset + va.freq));
            }
            rank_offset += va.freq;
        }
        let n_ranks = ranks_for_cause.len();
        let cutoff_val = ranks_for_cause[((n_ranks as f64) * cfg.cutoff_pos) as usize];
        cutoffs.insert(cause, cutoff_val);

        let mut like = Vec::with_capacity(6);
        like.push(0.0);
        for &thre in &thresholds {
            like.push(ranks_for_cause[((n_ranks as f64) * thre) as usize] as f64);
        }
        like.push((cutoff_val as f64).min(overall_cutoff));
        like.push(n_uniform as f64);
        likelihoods.insert(cause, like);
    }

    let model = Arc::new(TrainedModuleModel {
        cause_list,
        tariffs,
        uniform_len: n_uniform,
        uniform_scores,
        cutoffs,
        likelihoods,
    });
    cache.lock().unwrap().insert(key, Arc::clone(&model));
    Ok(model)
}

#[derive(Debug, Clone)]
struct ScoredVaRecord {
    sid: String,
    age: String,
    sex: String,
    cause: i32,
    cause34: Option<i32>,
    cause34_name: String,
    endorsements: HashSet<String>,
    scores: BTreeMap<i32, (f64, bool)>, // (score, had_endorsed_symptom)
    external_ranks: BTreeMap<i32, f64>,
    ranks: BTreeMap<i32, f64>,
    masked: HashMap<i32, HashSet<u8>>,
    rules: i32,
    likelihoods: Vec<(i32, usize)>,
}

const MASK_CENSORED: u8 = 1;
const MASK_EPI: u8 = 2;
const MASK_DEMOG: u8 = 3;
const MASK_OVERALL: u8 = 4;
const MASK_CAUSE_RANK: u8 = 5;
const MASK_SCORE: u8 = 6;

fn find_key_symptom(
    tariffs: &BTreeMap<i32, Vec<(String, f64)>>,
    cause_reduction: &[(i32, i32)],
    cause34: i32,
    endorsements: &HashSet<String>,
    rule_symptoms: &[(i32, &str)],
) -> Option<String> {
    if let Some(&(_, r_sym)) = rule_symptoms.iter().find(|&&(c, _)| c == cause34) {
        return Some(r_sym.to_string());
    }
    let causes46s: Vec<i32> = cause_reduction
        .iter()
        .filter_map(|&(c46, c34)| if c34 == cause34 { Some(c46) } else { None })
        .collect();
    let mut values: Vec<(String, f64)> = Vec::new();
    for cause46 in causes46s {
        if let Some(symp_list) = tariffs.get(&cause46) {
            for (symptom, tariff) in symp_list {
                if !endorsements.contains(symptom) || *tariff <= 0.0 {
                    continue;
                }
                if let Some(pos) = values.iter().position(|(s, _)| s == symptom) {
                    if values[pos].1 < *tariff {
                        continue;
                    } else {
                        values[pos].1 = *tariff;
                    }
                } else {
                    values.push((symptom.clone(), *tariff));
                }
            }
        }
    }
    if !values.is_empty() {
        values.sort_by(|a, b| a.1.total_cmp(&b.1));
        return values.pop().map(|(s, _)| s);
    }
    None
}

fn calc_age_bin(module: &str, age_str: &str) -> Option<i32> {
    let age = safe_float(Some(age_str));
    if module == "neonate" {
        if age < 7.0 {
            Some(0)
        } else if age <= 28.0 {
            Some(7)
        } else {
            None
        }
    } else if age == 0.0 {
        None
    } else if age < 1.0 {
        Some(0)
    } else if age < 5.0 {
        Some(1)
    } else if age <= 80.0 {
        Some(((age / 5.0) as i32) * 5)
    } else {
        Some(80)
    }
}

fn load_undetermined_weights(
    module: &str,
    iso3: Option<&str>,
    short_form: bool,
    hce: bool,
) -> Result<HashMap<(i32, i32), Vec<(String, f64)>>, SmartVaError> {
    let mut weights: HashMap<(i32, i32), Vec<(String, f64)>> = HashMap::new();
    let Some(country) = iso3.filter(|s| !s.is_empty() && *s != "Unknown") else {
        return Ok(weights);
    };
    let instrument = if short_form { "short" } else { "full" };
    let weight_key = format!("{}_hce{}", instrument, if hce { 1 } else { 0 });
    let Some(csv_str) = data::embedded_undetermined_weights_csv(module) else {
        return Ok(weights);
    };
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_str.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let idx_of = |name: &str| headers.iter().position(|h| h == name);
    let (Some(i_iso), Some(i_age), Some(i_sex), Some(i_cause), Some(i_w)) = (
        idx_of("iso3"),
        idx_of("age"),
        idx_of("sex"),
        idx_of("gs_text34"),
        idx_of(&weight_key),
    ) else {
        return Ok(weights);
    };

    for rec_res in rdr.records() {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        if rec.get(i_iso).unwrap_or("") != country {
            continue;
        }
        let age: i32 = rec.get(i_age).unwrap_or("0").parse().unwrap_or(0);
        let sex: i32 = rec.get(i_sex).unwrap_or("0").parse().unwrap_or(0);
        let cause = rec.get(i_cause).unwrap_or("").to_string();
        let w: f64 = rec.get(i_w).unwrap_or("0").parse().unwrap_or(0.0);
        weights.entry((age, sex)).or_default().push((cause, w));
    }
    Ok(weights)
}

struct TariffModuleResult {
    model: Arc<TrainedModuleModel>,
    user_data: Vec<ScoredVaRecord>,
    cause_counts: BTreeMap<String, f64>,
    counts_by_sex: HashMap<i32, BTreeMap<String, f64>>,
    pred_rows: Vec<Vec<String>>,
    mp_rows: Vec<Vec<String>>,
    predictions: Vec<SmartVaPrediction>,
}

fn tariff_prep_in_memory(
    module: &str,
    symptom_rows: &[Row],
    short_form: bool,
    options: &SmartVaRunOptions,
    iso3: Option<&str>,
    loggers: &mut ReportLoggers,
) -> Result<TariffModuleResult, SmartVaError> {
    let cfg = get_tariff_config(module);
    let model = get_or_build_trained_model(module, options.hce, options.freetext, short_form)?;

    let drop_keys: HashSet<&str> =
        HashSet::from(["sid", "real_age", "real_gender", "cause", "restricted"]);

    let mut user_data: Vec<ScoredVaRecord> = Vec::with_capacity(symptom_rows.len());
    for row in symptom_rows {
        let endorsements: HashSet<String> = row
            .iter()
            .filter_map(|(k, v)| {
                if !drop_keys.contains(k.as_str()) && safe_float(Some(v)) != 0.0 {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();
        let mut scores = BTreeMap::new();
        for (&cause, symp_list) in &model.tariffs {
            let mut sum = 0.0;
            let mut any_endorsed = false;
            for (symp, w) in symp_list {
                if endorsements.contains(symp) {
                    sum += *w;
                    any_endorsed = true;
                }
            }
            scores.insert(cause, (sum, any_endorsed));
        }
        let mut masked: HashMap<i32, HashSet<u8>> = HashMap::new();
        for tok in row
            .get("restricted")
            .map(|s| s.as_str())
            .unwrap_or("")
            .split_whitespace()
        {
            let c = safe_int(Some(tok));
            masked.entry(c).or_default().insert(MASK_CENSORED);
        }
        let rules = safe_int(row.get("cause").map(|s| s.as_str()));
        user_data.push(ScoredVaRecord {
            sid: row.get("sid").cloned().unwrap_or_default(),
            age: row.get("real_age").cloned().unwrap_or_default(),
            sex: row.get("real_gender").cloned().unwrap_or_default(),
            cause: 0,
            cause34: None,
            cause34_name: String::new(),
            endorsements,
            scores,
            external_ranks: BTreeMap::new(),
            ranks: BTreeMap::new(),
            masked,
            rules,
            likelihoods: Vec::new(),
        });
    }

    // generate_cause_rankings
    for va in &mut user_data {
        for &cause in &model.cause_list {
            let u_scores = &model.uniform_scores[&cause];
            let s = va.scores[&cause].0;
            let gt = u_scores.partition_point(|&x| x < s);
            let lt = u_scores.partition_point(|&x| x <= s);
            let avg_rank = (u_scores.len() as f64) - (gt as f64) + ((gt as f64 - lt as f64) / 2.0);
            let r = avg_rank + 0.5;
            va.external_ranks.insert(cause, r);
            va.ranks.insert(cause, r);
        }
    }

    // mask_ranks
    let lowest_rank = (model.uniform_len as f64) + 0.5;
    let overall_rank_cutoff = (model.uniform_len as f64) * cfg.uniform_list_pos;
    let min_overrides: HashMap<i32, f64> = cfg.min_cause_score_overrides.iter().copied().collect();

    for va in &mut user_data {
        for (&cause, &(score, _)) in &va.scores {
            let min_score = min_overrides
                .get(&cause)
                .copied()
                .unwrap_or(cfg.min_cause_score_default);
            if score <= min_score {
                va.masked.entry(cause).or_default().insert(MASK_SCORE);
            }
        }
        let age_f = safe_float(Some(&va.age));
        let sex_f = safe_float(Some(&va.sex));
        for &(cond, causes) in cfg.cause_conditions {
            let ok = eval_ldap(cond, &|k| match k {
                "age" => age_f,
                "sex" => sex_f,
                _ => 0.0,
            });
            if !ok {
                for &c in causes {
                    va.masked.entry(c).or_default().insert(MASK_DEMOG);
                }
            }
        }
        if !options.hiv {
            for &c in cfg.hiv_causes {
                va.masked.entry(c).or_default().insert(MASK_EPI);
            }
        }
        if !options.malaria {
            for &c in cfg.malaria_causes {
                va.masked.entry(c).or_default().insert(MASK_EPI);
            }
        }
        for &cause in &model.cause_list {
            let r = va.ranks[&cause];
            if r > (model.cutoffs[&cause] as f64) {
                va.masked.entry(cause).or_default().insert(MASK_CAUSE_RANK);
            }
            if r > overall_rank_cutoff {
                va.masked.entry(cause).or_default().insert(MASK_OVERALL);
            }
        }
        for &cause in va.masked.keys() {
            if va.ranks.contains_key(&cause) {
                va.ranks.insert(cause, lowest_rank);
            }
        }
    }

    // predict
    let causes34_map: HashMap<i32, &str> = cfg.causes.iter().copied().collect();
    let causes46_map: HashMap<i32, &str> = cfg.causes46.iter().copied().collect();
    let reduction_map: HashMap<i32, i32> = cfg.cause_reduction.iter().copied().collect();

    for (i, va) in user_data.iter_mut().enumerate() {
        let is_rule_masked = va.masked.get(&va.rules).is_some_and(|m| {
            m.contains(&MASK_EPI) || m.contains(&MASK_DEMOG) || m.contains(&MASK_CENSORED)
        });
        if va.rules != 0 && causes46_map.contains_key(&va.rules) && !is_rule_masked {
            va.cause = va.rules;
        } else {
            let unique_ranks: HashSet<u64> = va.ranks.values().map(|v| v.to_bits()).collect();
            if unique_ranks.len() > 1 {
                let best_rank = va
                    .ranks
                    .values()
                    .copied()
                    .fold(f64::INFINITY, f64::min);
                let mut predictions: Vec<i32> = va
                    .ranks
                    .iter()
                    .filter_map(|(&c, &r)| {
                        let is_masked = va.masked.get(&c).is_some_and(|m| !m.is_empty());
                        if r == best_rank && !is_masked {
                            Some(c)
                        } else {
                            None
                        }
                    })
                    .collect();
                predictions.sort_unstable();
                if let Some(&first_c) = predictions.first() {
                    va.cause = first_c;
                }
                if predictions.len() > 1 {
                    let names: Vec<&str> =
                        predictions.iter().map(|c| causes46_map[c]).collect();
                    let title_mod = {
                        let mut s = cfg.age_group.to_string();
                        if let Some(r) = s.get_mut(0..1) {
                            r.make_ascii_uppercase();
                        }
                        s
                    };
                    let msg = format!(
                        "SID: {} ({} row {}) had multiple causes predicted to be equally likely: {:?}, using first listed.",
                        va.sid, title_mod, i, names
                    );
                    loggers.warnings.push(msg.clone());
                    loggers.prediction.push(msg);
                }
            }
        }
        va.cause34 = reduction_map.get(&va.cause).copied();
        va.cause34_name = va
            .cause34
            .and_then(|c34| causes34_map.get(&c34).copied())
            .unwrap_or("Undetermined")
            .to_string();
    }

    // determine_likelihood
    for va in &mut user_data {
        if va.cause == 0 {
            continue;
        }
        let mut ordered: Vec<(i32, f64)> = va
            .ranks
            .iter()
            .filter_map(|(&c, &r)| {
                let is_masked = va.masked.get(&c).is_some_and(|m| !m.is_empty());
                if !is_masked && c != va.cause {
                    Some((c, r))
                } else {
                    None
                }
            })
            .collect();
        ordered.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let top_rank = va.ranks.get(&va.cause).copied().unwrap_or(0.0);
        ordered.insert(0, (va.cause, top_rank));

        let mut pred34: Vec<i32> = Vec::new();
        let mut like_map: HashMap<i32, usize> = HashMap::new();
        let mut prev_likelihood: usize = 0;

        for (cause46, rank) in ordered {
            let Some(&cause34) = reduction_map.get(&cause46) else {
                continue;
            };
            if !pred34.contains(&cause34) {
                pred34.push(cause34);
            }
            let thresh = &model.likelihoods[&cause46];
            let likelihood46 = if cause46 == va.cause && va.cause == va.rules {
                0
            } else if rank >= *thresh.last().unwrap() {
                thresh.len() - 2
            } else {
                thresh.partition_point(|&x| x <= rank).saturating_sub(1)
            };

            let likelihood34 = if let Some(&prev_34) = like_map.get(&cause34) {
                likelihood46.min(prev_34)
            } else {
                likelihood46.max(prev_likelihood)
            };
            prev_likelihood = likelihood34;
            like_map.insert(cause34, likelihood34);
        }
        va.likelihoods = pred34.into_iter().map(|c| (c, like_map[&c])).collect();
    }

    // calculate_csmf
    let undet_weights = load_undetermined_weights(cfg.age_group, iso3, short_form, options.hce)?;
    let mut cause_counts: BTreeMap<String, f64> = BTreeMap::new();
    let mut counts_by_sex: HashMap<i32, BTreeMap<String, f64>> =
        HashMap::from([(1, BTreeMap::new()), (2, BTreeMap::new())]);

    for va in &user_data {
        let sex = safe_int(Some(&va.sex));
        if va.cause34_name == "Undetermined" && !undet_weights.is_empty() {
            let age_bin = calc_age_bin(cfg.age_group, &va.age).unwrap_or(-1);
            let redist = undet_weights
                .get(&(age_bin, sex))
                .or_else(|| undet_weights.get(&(99, 3)));
            if let Some(entries) = redist {
                for (c_name, w) in entries {
                    *cause_counts.entry(c_name.clone()).or_insert(0.0) += *w;
                    if let Some(sex_map) = counts_by_sex.get_mut(&sex) {
                        *sex_map.entry(c_name.clone()).or_insert(0.0) += *w;
                    }
                }
            }
        } else {
            *cause_counts.entry(va.cause34_name.clone()).or_insert(0.0) += 1.0;
            if let Some(sex_map) = counts_by_sex.get_mut(&sex) {
                *sex_map.entry(va.cause34_name.clone()).or_insert(0.0) += 1.0;
            }
        }
    }

    let mut drop_causes: Vec<i32> = Vec::new();
    if !options.hiv {
        drop_causes.extend_from_slice(cfg.hiv_causes);
    }
    if !options.malaria {
        drop_causes.extend_from_slice(cfg.malaria_causes);
    }
    let mut male_drops: Vec<i32> = Vec::new();
    male_drops.extend_from_slice(cfg.female_causes);
    male_drops.extend_from_slice(cfg.maternal_causes);
    let female_drops: Vec<i32> = cfg.male_causes.to_vec();

    for c46 in drop_causes {
        if let Some(&c34) = reduction_map.get(&c46) {
            if let Some(&gs_text34) = causes34_map.get(&c34) {
                if cause_counts.contains_key(gs_text34) {
                    cause_counts.remove(gs_text34);
                    counts_by_sex.get_mut(&1).unwrap().remove(gs_text34);
                    counts_by_sex.get_mut(&2).unwrap().remove(gs_text34);
                }
            }
        }
    }
    for c46 in male_drops {
        if let Some(&c34) = reduction_map.get(&c46) {
            if let Some(&gs_text34) = causes34_map.get(&c34) {
                counts_by_sex.get_mut(&1).unwrap().remove(gs_text34);
            }
        }
    }
    for c46 in female_drops {
        if let Some(&c34) = reduction_map.get(&c46) {
            if let Some(&gs_text34) = causes34_map.get(&c34) {
                counts_by_sex.get_mut(&2).unwrap().remove(gs_text34);
            }
        }
    }

    // write_predictions (mutates va.age to "" if 0 for adult/child)
    let mut pred_rows = Vec::with_capacity(user_data.len() + 1);
    pred_rows.push(vec![
        "sid".to_string(),
        "cause".to_string(),
        "cause34".to_string(),
        "age".to_string(),
        "sex".to_string(),
    ]);
    for va in &mut user_data {
        if safe_float(Some(&va.age)) == 0.0 && (cfg.age_group == "adult" || cfg.age_group == "child")
        {
            va.age.clear();
        }
        pred_rows.push(vec![
            va.sid.clone(),
            va.cause34.map(|c| c.to_string()).unwrap_or_default(),
            va.cause34_name.clone(),
            va.age.clone(),
            va.sex.clone(),
        ]);
    }

    // write_multiple_predictions_csv and build SmartVaPrediction
    let likelihood_names = ["Very Likely", "Likely", "Somewhat Likely", "Possible"];
    let symp_desc_map: HashMap<&str, &str> = cfg.symptom_descriptions.iter().copied().collect();
    let mut symp_order: HashMap<&str, usize> = HashMap::new();
    for (i, &(_, desc)) in cfg.symptom_descriptions.iter().enumerate() {
        symp_order.entry(desc).or_insert(i);
    }

    let mut mp_rows: Vec<Vec<String>> = Vec::with_capacity(user_data.len() + 1);
    mp_rows.push(vec![
        "sid".to_string(),
        "age".to_string(),
        "sex".to_string(),
        "cause1".to_string(),
        "likelihood1".to_string(),
        "key_symptom1".to_string(),
        "cause2".to_string(),
        "likelihood2".to_string(),
        "key_symptom2".to_string(),
        "cause3".to_string(),
        "likelihood3".to_string(),
        "key_symptom3".to_string(),
        "all_symptoms".to_string(),
    ]);

    let mut predictions = Vec::with_capacity(user_data.len());

    for va in &user_data {
        let sex_label = match va.sex.as_str() {
            "1" => "Male",
            "2" => "Female",
            _ => "Missing",
        };
        let mut csv_row = vec![va.sid.clone(), va.age.clone(), sex_label.to_string()];
        let mut slots: [(Option<String>, Option<String>, Option<String>); 3] =
            [(None, None, None), (None, None, None), (None, None, None)];

        if !va.likelihoods.is_empty() {
            for (c_idx, &(cause34, like_idx)) in va.likelihoods.iter().enumerate() {
                if c_idx == 3 {
                    break;
                }
                let cause_name = causes34_map
                    .get(&cause34)
                    .copied()
                    .unwrap_or("Undetermined");
                let like_name = likelihood_names[like_idx.min(likelihood_names.len() - 1)];
                let key_sym = find_key_symptom(
                    &model.tariffs,
                    cfg.cause_reduction,
                    cause34,
                    &va.endorsements,
                    cfg.rule_key_symptoms,
                );
                let sym_desc = key_sym
                    .as_deref()
                    .and_then(|s| symp_desc_map.get(s).copied())
                    .unwrap_or("");
                csv_row.push(cause_name.to_string());
                csv_row.push(like_name.to_string());
                csv_row.push(sym_desc.to_string());
                slots[c_idx] = (
                    Some(cause_name.to_string()),
                    Some(like_name.to_string()),
                    if sym_desc.is_empty() {
                        None
                    } else {
                        Some(sym_desc.to_string())
                    },
                );
            }
        } else {
            csv_row.push("Undetermined".to_string());
            slots[0].0 = Some("Undetermined".to_string());
        }
        while csv_row.len() < 12 {
            csv_row.push(String::new());
        }
        let mut endorsed_descs: Vec<&str> = va
            .endorsements
            .iter()
            .filter_map(|s| symp_desc_map.get(s.as_str()).copied())
            .collect();
        endorsed_descs.sort_by_key(|d| symp_order.get(d).copied().unwrap_or(usize::MAX));
        let all_symp_str = endorsed_descs.join("; ");
        csv_row.push(all_symp_str.clone());
        mp_rows.push(csv_row);

        let icd = icds::get_icd(cfg.age_group, &va.cause34_name)
            .unwrap_or("")
            .to_string();
        predictions.push(SmartVaPrediction {
            sid: va.sid.clone(),
            module: Some(cfg.age_group.to_string()),
            national_id: String::new(),
            name: String::new(),
            name2: String::new(),
            surname: String::new(),
            surname2: String::new(),
            geography1: String::new(),
            geography2: String::new(),
            geography3: String::new(),
            geography4: String::new(),
            geography5: String::new(),
            cause34: va.cause34_name.clone(),
            cause_list_number: va.cause34.map(|c| c.to_string()).unwrap_or_default(),
            icd10: icd,
            age: if va.age.is_empty() {
                None
            } else {
                va.age.parse::<f64>().ok()
            },
            sex: va.sex.parse::<i32>().ok(),
            birth_date: String::new(),
            death_date: String::new(),
            interview_date: String::new(),
            cause1: slots[0].0.clone(),
            likelihood1: slots[0].1.clone(),
            key_symptom1: slots[0].2.clone(),
            cause2: slots[1].0.clone(),
            likelihood2: slots[1].1.clone(),
            key_symptom2: slots[1].2.clone(),
            cause3: slots[2].0.clone(),
            likelihood3: slots[2].1.clone(),
            key_symptom3: slots[2].2.clone(),
            all_symptoms: Some(all_symp_str),
        });
    }

    Ok(TariffModuleResult {
        model,
        user_data,
        cause_counts,
        counts_by_sex,
        pred_rows,
        mp_rows,
        predictions,
    })
}

fn run_tariff_prep(
    output_dir: &Path,
    intermediate_dir: &Path,
    module: &str,
    short_form: bool,
    options: &SmartVaRunOptions,
    iso3: Option<&str>,
    loggers: &mut ReportLoggers,
) -> Result<Vec<SmartVaPrediction>, SmartVaError> {
    let cfg = get_tariff_config(module);
    let symptom_path = intermediate_dir.join(format!("{}-symptom.csv", cfg.age_group));
    let (_, symptom_rows) = read_csv_matrix(&symptom_path)?;

    let res = tariff_prep_in_memory(module, &symptom_rows, short_form, options, iso3, loggers)?;

    // Write {module}-cutoffs.txt
    let cutoffs_path = intermediate_dir.join(format!("{}-cutoffs.txt", cfg.age_group));
    {
        let mut f = File::create(&cutoffs_path)
            .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        for &c in &res.model.cause_list {
            writeln!(f, "{} : {}", c, res.model.cutoffs[&c])
                .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        }
    }

    // write external-ranks
    write_tariff_intermediate_ranks(
        &intermediate_dir.join(format!("{}-external-ranks.csv", cfg.age_group)),
        &res.model.cause_list,
        &res.user_data,
        true,
    )?;

    write_csv_rows(
        &output_dir.join(format!("{}-predictions.csv", cfg.age_group)),
        &res.pred_rows,
    )?;

    let mp_csv_path = output_dir.join(format!("{}-likelihoods.csv", cfg.age_group));
    write_csv_rows(&mp_csv_path, &res.mp_rows)?;

    // write_csmf
    write_raw_csmf_file(
        &output_dir.join(format!("{}-csmf.csv", cfg.age_group)),
        &res.cause_counts,
    )?;
    write_raw_csmf_file(
        &output_dir.join(format!("{}-male-csmf.csv", cfg.age_group)),
        &res.counts_by_sex[&1],
    )?;
    write_raw_csmf_file(
        &output_dir.join(format!("{}-female-csmf.csv", cfg.age_group)),
        &res.counts_by_sex[&2],
    )?;

    // write tariff-scores and tariff-ranks
    write_tariff_intermediate_scores(
        &intermediate_dir.join(format!("{}-tariff-scores.csv", cfg.age_group)),
        &res.model.cause_list,
        &res.user_data,
    )?;
    write_tariff_intermediate_ranks(
        &intermediate_dir.join(format!("{}-tariff-ranks.csv", cfg.age_group)),
        &res.model.cause_list,
        &res.user_data,
        false,
    )?;

    Ok(res.predictions)
}

fn write_raw_csmf_file(path: &Path, counts: &BTreeMap<String, f64>) -> Result<(), SmartVaError> {
    let total: f64 = counts.values().sum();
    let mut rows = Vec::with_capacity(counts.len() + 1);
    rows.push(vec!["cause".to_string(), "CSMF".to_string()]);
    if total > 0.0 {
        for (k, &v) in counts {
            rows.push(vec![k.clone(), py_float_str(v / total)]);
        }
    }
    write_csv_rows(path, &rows)
}

fn build_tariff_scores_rows(
    cause_list: &[i32],
    user_data: &[ScoredVaRecord],
) -> Vec<Vec<String>> {
    let mut rows = Vec::with_capacity(user_data.len() + 1);
    let mut hdr = Vec::with_capacity(cause_list.len() + 1);
    hdr.push("sid".to_string());
    hdr.extend(cause_list.iter().map(|c| c.to_string()));
    rows.push(hdr);
    for va in user_data {
        let mut r = Vec::with_capacity(cause_list.len() + 1);
        r.push(va.sid.clone());
        for c in cause_list {
            let (score, had_endorsed) = va.scores[c];
            if !had_endorsed {
                r.push("0".to_string());
            } else {
                r.push(py_float_str(score));
            }
        }
        rows.push(r);
    }
    rows
}

fn write_tariff_intermediate_scores(
    path: &Path,
    cause_list: &[i32],
    user_data: &[ScoredVaRecord],
) -> Result<(), SmartVaError> {
    let rows = build_tariff_scores_rows(cause_list, user_data);
    write_csv_rows(path, &rows)
}

fn build_tariff_ranks_rows(
    cause_list: &[i32],
    user_data: &[ScoredVaRecord],
    external: bool,
) -> Vec<Vec<String>> {
    let mut rows = Vec::with_capacity(user_data.len() + 1);
    let mut hdr = Vec::with_capacity(cause_list.len() + 1);
    hdr.push("sid".to_string());
    hdr.extend(cause_list.iter().map(|c| c.to_string()));
    rows.push(hdr);
    for va in user_data {
        let mut r = Vec::with_capacity(cause_list.len() + 1);
        r.push(va.sid.clone());
        let map = if external {
            &va.external_ranks
        } else {
            &va.ranks
        };
        for c in cause_list {
            r.push(py_float_str(map[c]));
        }
        rows.push(r);
    }
    rows
}

fn write_tariff_intermediate_ranks(
    path: &Path,
    cause_list: &[i32],
    user_data: &[ScoredVaRecord],
    external: bool,
) -> Result<(), SmartVaError> {
    let rows = build_tariff_ranks_rows(cause_list, user_data, external);
    write_csv_rows(path, &rows)
}

fn make_output_date(year: &str, month: &str, day: &str) -> String {
    let (Some(yf), Some(mf), Some(df)) = (
        parse_py_float(Some(year)),
        parse_py_float(Some(month)),
        parse_py_float(Some(day)),
    ) else {
        return String::new();
    };
    let cur_year = chrono::Local::now().year() as f64;
    if (1880.0..=cur_year).contains(&yf) && (1.0..=12.0).contains(&mf) && (1.0..=31.0).contains(&df)
    {
        format!("{}-{:02}-{:02}", yf as i32, mf as i32, df as i32)
    } else {
        String::new()
    }
}

fn is_invalid_interview_date(s: &str) -> bool {
    s.chars().all(|c| matches!(c, ':' | '.' | '0'))
}

fn bin_ages_for_table(module: &str, age: f64) -> &'static str {
    match module {
        "neonate" => {
            if age < 7.0 / 365.0 {
                " 00-07 days"
            } else {
                " 08-28 days"
            }
        }
        "child" => {
            if age < 1.0 {
                " 29 days - <1 year"
            } else if age < 5.0 {
                "01-04 years"
            } else {
                "05-11 years"
            }
        }
        _ => {
            if age < 20.0 {
                "12-19 years"
            } else if age >= 80.0 {
                "80+"
            } else {
                match (age / 10.0) as i32 {
                    2 => "20-29 years",
                    3 => "30-39 years",
                    4 => "40-49 years",
                    5 => "50-59 years",
                    6 => "60-69 years",
                    _ => "70-79 years",
                }
            }
        }
    }
}

fn get_five_year_age_group(age_str: &str) -> String {
    let Some(age) = parse_py_float(Some(age_str)) else {
        return "unknown".to_string();
    };
    if age < 1.0 {
        " <1 yr".to_string()
    } else if age < 5.0 {
        "1-4 yr".to_string()
    } else if age >= 85.0 {
        "85+ yr".to_string()
    } else {
        let lower = ((age / 5.0) as i32) * 5;
        let upper = lower + 4;
        format!("{}-{} yr", lower, upper)
    }
}

fn round_py(val: f64, digits: i32) -> String {
    let scale = 10_f64.powi(digits);
    let r = (val * scale).round_ties_even() / scale;
    py_float_str(r)
}

fn run_output_prep(
    working_dir: &Path,
    intermediate_dir: &Path,
    short_form: bool,
    free_text: bool,
    hce: bool,
    loggers: &ReportLoggers,
) -> Result<(), SmartVaError> {
    let f1 = working_dir.join("1-individual-cause-of-death");
    let f2 = working_dir.join("2-csmf");
    let f3 = working_dir.join("3-graphs-and-tables");
    let f4 = working_dir.join("4-monitoring-and-quality");
    let f4_inter = f4.join("intermediate-files");
    for dir in [&f1, &f2, &f3, &f4, &f4_inter] {
        fs::create_dir_all(dir).map_err(|e| SmartVaError::IoError(e.to_string()))?;
    }

    // Folder 1: individual-cause-of-death.csv and {module}-predictions.csv
    let f1_headers: Vec<String> = [
        "sid",
        "national_id",
        "name",
        "name2",
        "surname",
        "surname2",
        "geography1",
        "geography2",
        "geography3",
        "geography4",
        "geography5",
        "cause34",
        "cause list #",
        "icd10",
        "age",
        "sex",
        "birth_date",
        "death_date",
        "interview_date",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let mut all_f1_rows = vec![f1_headers.clone()];
    let mut predictions_by_mod: HashMap<&str, Vec<Row>> = HashMap::new();

    for module in MODULES {
        let pred_file = working_dir.join(format!("{}-predictions.csv", module));
        let raw_file = intermediate_dir.join(format!("{}-prepped.csv", module));
        if !(pred_file.exists() && raw_file.exists()) {
            predictions_by_mod.insert(module, Vec::new());
            continue;
        }
        let (_, pred_rows) = read_csv_matrix(&pred_file)?;
        let mut raw_rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_path(&raw_file)
            .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        let raw_hdrs: Vec<String> = raw_rdr
            .headers()
            .map_err(|e| SmartVaError::CsvError(e.to_string()))?
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r_idx = |name: &str| raw_hdrs.iter().position(|h| h == name);
        let i_interview = r_idx("interviewdate");
        let i_nat = r_idx("gen_6_7");
        let i_name = r_idx("gen_5_0");
        let i_name2 = r_idx("gen_5_0c");
        let i_sur = r_idx("gen_5_0a");
        let i_sur2 = r_idx("gen_5_0a2");
        let i_g1 = r_idx("gen_2_3a");
        let i_g2 = r_idx("gen_2_3b");
        let i_g3 = r_idx("gen_2_3c");
        let i_g4 = r_idx("gen_2_3d");
        let i_g5 = r_idx("gen_2_3e");
        let i_b1 = r_idx("gen_5_1a");
        let i_b2 = r_idx("gen_5_1b");
        let i_b3 = r_idx("gen_5_1c");
        let i_d1 = r_idx("gen_5_3a");
        let i_d2 = r_idx("gen_5_3b");
        let i_d3 = r_idx("gen_5_3c");
        let cell = |rec: &csv::StringRecord, opt_i: Option<usize>| -> String {
            opt_i.and_then(|i| rec.get(i)).unwrap_or("").to_string()
        };

        let mut mod_f1_rows = vec![f1_headers.clone()];

        for (raw_rec_res, pred_row) in raw_rdr.records().zip(pred_rows.iter()) {
            let raw_rec = raw_rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
            let mut interview_date = cell(&raw_rec, i_interview);
            if is_invalid_interview_date(&interview_date) {
                interview_date.clear();
            }
            let cause34_str = pred_row.get("cause34").map(|s| s.as_str()).unwrap_or("");
            let icd = icds::get_icd(module, cause34_str).unwrap_or("");
            let birth_date = make_output_date(
                &cell(&raw_rec, i_b1),
                &cell(&raw_rec, i_b2),
                &cell(&raw_rec, i_b3),
            );
            let death_date = make_output_date(
                &cell(&raw_rec, i_d1),
                &cell(&raw_rec, i_d2),
                &cell(&raw_rec, i_d3),
            );
            let r = vec![
                pred_row.get("sid").cloned().unwrap_or_default(),
                cell(&raw_rec, i_nat),
                cell(&raw_rec, i_name),
                cell(&raw_rec, i_name2),
                cell(&raw_rec, i_sur),
                cell(&raw_rec, i_sur2),
                cell(&raw_rec, i_g1),
                cell(&raw_rec, i_g2),
                cell(&raw_rec, i_g3),
                cell(&raw_rec, i_g4),
                cell(&raw_rec, i_g5),
                cause34_str.to_string(),
                pred_row.get("cause").cloned().unwrap_or_default(),
                icd.to_string(),
                pred_row.get("age").cloned().unwrap_or_default(),
                pred_row.get("sex").cloned().unwrap_or_default(),
                birth_date,
                death_date,
                interview_date,
            ];
            all_f1_rows.push(r.clone());
            mod_f1_rows.push(r);
        }
        write_csv_rows(&f1.join(format!("{}-predictions.csv", module)), &mod_f1_rows)?;
        predictions_by_mod.insert(module, pred_rows);
    }
    write_csv_rows(&f1.join("individual-cause-of-death.csv"), &all_f1_rows)?;

    // Folder 2: CSMFs
    let mut mod_csmfs: HashMap<&str, HashMap<&str, BTreeMap<String, f64>>> = HashMap::new();
    for module in MODULES {
        let cfg = get_tariff_config(module);
        let cause_num_by_name: HashMap<&str, i32> =
            cfg.causes.iter().map(|&(num, name)| (name, num)).collect();
        let mut csmf_map: HashMap<&str, BTreeMap<String, f64>> = HashMap::new();
        for (file_prefix, label) in [
            (module.to_string(), "both"),
            (format!("{}-male", module), "male"),
            (format!("{}-female", module), "female"),
        ] {
            let p = working_dir.join(format!("{}-csmf.csv", file_prefix));
            let mut inner = BTreeMap::new();
            if p.exists() {
                let mut rdr = csv::ReaderBuilder::new()
                    .has_headers(true)
                    .from_path(&p)
                    .map_err(|e| SmartVaError::IoError(e.to_string()))?;
                for rec_res in rdr.records() {
                    let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
                    let c = rec.get(0).unwrap_or("").to_string();
                    let v = safe_float(rec.get(1));
                    inner.insert(c, v);
                }
            }
            csmf_map.insert(label, inner);
        }
        let mut table = vec![vec![
            "cause34".to_string(),
            "cause list #".to_string(),
            "icd10".to_string(),
            "all".to_string(),
            "male".to_string(),
            "female".to_string(),
        ]];
        if let Some(both_map) = csmf_map.get("both") {
            for cause in both_map.keys() {
                let c_num = cause_num_by_name
                    .get(cause.as_str())
                    .map(|n| n.to_string())
                    .unwrap_or_default();
                let icd = icds::get_icd(module, cause).unwrap_or("").to_string();
                let all_s = both_map
                    .get(cause)
                    .map(|&v| round_py(v, 3))
                    .unwrap_or_else(|| "0.0".to_string());
                let male_s = csmf_map
                    .get("male")
                    .and_then(|m| m.get(cause))
                    .map(|&v| round_py(v, 3))
                    .unwrap_or_else(|| "0.0".to_string());
                let female_s = csmf_map
                    .get("female")
                    .and_then(|m| m.get(cause))
                    .map(|&v| round_py(v, 3))
                    .unwrap_or_else(|| "0.0".to_string());
                table.push(vec![
                    cause.clone(),
                    c_num,
                    icd,
                    all_s,
                    male_s,
                    female_s,
                ]);
            }
        }
        write_csv_rows(&f2.join(format!("{}-csmf.csv", module)), &table)?;
        mod_csmfs.insert(module, csmf_map);
    }

    let total_preds: usize = predictions_by_mod.values().map(|v| v.len()).sum();
    if total_preds > 0 {
        let total_f = total_preds as f64;
        let mut all_csmf: BTreeMap<String, HashMap<&str, f64>> = BTreeMap::new();
        for module in MODULES {
            let weight = (predictions_by_mod[module].len() as f64) / total_f;
            if let Some(m_csmf) = mod_csmfs.get(module) {
                for (&sex_lbl, values) in m_csmf {
                    for (cause, &val) in values {
                        *all_csmf
                            .entry(cause.clone())
                            .or_default()
                            .entry(sex_lbl)
                            .or_insert(0.0) += val * weight;
                    }
                }
            }
        }
        let mut table = vec![vec![
            "cause34".to_string(),
            "icd10".to_string(),
            "all".to_string(),
            "male".to_string(),
            "female".to_string(),
        ]];
        for (cause, sex_vals) in &all_csmf {
            let icd = icds::get_any_icd(cause).unwrap_or("").to_string();
            table.push(vec![
                cause.clone(),
                icd,
                sex_vals
                    .get("both")
                    .map(|&v| round_py(v, 3))
                    .unwrap_or_else(|| "0".to_string()),
                sex_vals
                    .get("male")
                    .map(|&v| round_py(v, 3))
                    .unwrap_or_else(|| "0".to_string()),
                sex_vals
                    .get("female")
                    .map(|&v| round_py(v, 3))
                    .unwrap_or_else(|| "0".to_string()),
            ]);
        }
        write_csv_rows(&f2.join("csmf.csv"), &table)?;

        // gbd-level1-csmf.csv
        let mut gbd_csmf: BTreeMap<&str, f64> = BTreeMap::new();
        for module in MODULES {
            let weight = (predictions_by_mod[module].len() as f64) / total_f;
            if let Some(both_vals) = mod_csmfs.get(module).and_then(|m| m.get("both")) {
                for (cause, &val) in both_vals {
                    if let Some(gbd_code) = gbd_causes::get_gbd_level1(module, cause) {
                        *gbd_csmf.entry(gbd_code).or_insert(0.0) += val * weight;
                    }
                }
            }
        }
        let mut gbd_table = vec![vec![
            "cause".to_string(),
            "cause_name".to_string(),
            "CSMF".to_string(),
        ]];
        for (code, val) in gbd_csmf {
            let name = gbd_causes::GBD_LEVEL1_CAUSE_NAMES
                .iter()
                .find(|&&(k, _)| k == code)
                .map(|&(_, v)| v)
                .unwrap_or("");
            gbd_table.push(vec![code.to_string(), name.to_string(), round_py(val, 3)]);
        }
        write_csv_rows(&f2.join("gbd-level1-csmf.csv"), &gbd_table)?;
    }

    // Folder 3: causes-of-death.csv
    let age_groups_order: [(&str, &str); 14] = [
        ("neonate0", " 00 days"),
        ("neonate1", " 00-07 days"),
        ("neonate2", " 08-28 days"),
        ("child1", " 29 days - <1 year"),
        ("child2", "01-04 years"),
        ("child3", "05-11 years"),
        ("adult1", "12-19 years"),
        ("adult2", "20-29 years"),
        ("adult3", "30-39 years"),
        ("adult4", "40-49 years"),
        ("adult5", "50-59 years"),
        ("adult6", "60-69 years"),
        ("adult7", "70-79 years"),
        ("adult8", "80+"),
    ];
    let mut cod_counts: HashMap<(&str, &str, String), usize> = HashMap::new();
    let mut mod_causes: HashMap<&str, BTreeSet<String>> = HashMap::new();
    for module in MODULES {
        mod_causes.insert(module, BTreeSet::new());
        for row in &predictions_by_mod[module] {
            let cause = row.get("cause34").cloned().unwrap_or_default();
            mod_causes.get_mut(module).unwrap().insert(cause.clone());
            let age_grp = if cause == "Stillbirth" {
                " 00 days"
            } else {
                bin_ages_for_table(module, safe_float(row.get("age").map(|s| s.as_str())))
            };
            let sex_lbl = match safe_int(row.get("sex").map(|s| s.as_str())) {
                1 => "Male",
                2 => "Female",
                _ => "Missing",
            };
            *cod_counts.entry((age_grp, sex_lbl, cause)).or_insert(0) += 1;
        }
    }
    let mut cod_table = vec![vec![
        "module".to_string(),
        "cause34".to_string(),
        "age".to_string(),
        "sex".to_string(),
        "counts".to_string(),
    ]];
    for (age_i, age_grp) in age_groups_order {
        let module = &age_i[..age_i.len() - 1];
        for sex_lbl in ["Male", "Female", "Missing"] {
            if let Some(c_set) = mod_causes.get(module) {
                for cause in c_set {
                    if let Some(&cnt) = cod_counts.get(&(age_grp, sex_lbl, cause.clone())) {
                        if cnt > 0 {
                            cod_table.push(vec![
                                module.to_string(),
                                cause.clone(),
                                age_grp.to_string(),
                                sex_lbl.to_string(),
                                cnt.to_string(),
                            ]);
                        }
                    }
                }
            }
        }
    }
    write_csv_rows(&f3.join("causes-of-death.csv"), &cod_table)?;

    // Folder 4: Monitoring and Quality
    let yes_no_q_set: HashSet<&str> = codebook::YES_NO_QUESTIONS.iter().copied().collect();
    let yes_no_l_map: HashMap<i32, &str> = codebook::YES_NO_LABELS.iter().copied().collect();
    let dur_cat_set: HashSet<&str> = codebook::DURATION_CATEORICALS.iter().copied().collect();
    let dur_l_map: HashMap<i32, &str> = codebook::DURATION_LABELS.iter().copied().collect();
    let word_col_set: HashSet<&str> = codebook::WORD_COLUMNS.iter().copied().collect();
    let word_l_map: HashMap<i32, &str> = codebook::WORD_LABELS.iter().copied().collect();
    let cat_l_map: HashMap<&str, HashMap<i32, &str>> = codebook::CATEGORICAL_LABELS
        .iter()
        .map(|&(col, pairs)| (col, pairs.iter().copied().collect()))
        .collect();

    for module in MODULES {
        // 1. _recode_prepped_files -> {module}-raw-data.csv (streamed row-by-row)
        let prepped_file = intermediate_dir.join(format!("{}-prepped.csv", module));
        if prepped_file.exists() {
            let mut rdr = csv::ReaderBuilder::new()
                .has_headers(true)
                .flexible(true)
                .from_path(&prepped_file)
                .map_err(|e| SmartVaError::IoError(e.to_string()))?;
            let p_headers: Vec<String> = rdr
                .headers()
                .map_err(|e| SmartVaError::CsvError(e.to_string()))?
                .iter()
                .map(|s| s.to_string())
                .collect();
            let raw_out_path = f4_inter.join(format!("{}-raw-data.csv", module));
            let file = File::create(&raw_out_path)
                .map_err(|e| SmartVaError::IoError(e.to_string()))?;
            let mut wtr = csv::WriterBuilder::new()
                .terminator(csv::Terminator::CRLF)
                .from_writer(BufWriter::new(file));
            wtr.write_record(&p_headers)
                .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
            for rec_res in rdr.records() {
                let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
                wtr.write_record(p_headers.iter().enumerate().map(|(i, k)| {
                    let v = rec.get(i).unwrap_or("");
                    let iv = safe_int(Some(v));
                    let k_str = k.as_str();
                    if yes_no_q_set.contains(k_str) {
                        yes_no_l_map.get(&iv).copied().unwrap_or("")
                    } else if dur_cat_set.contains(k_str) {
                        dur_l_map.get(&iv).copied().unwrap_or("")
                    } else if word_col_set.contains(k_str) {
                        word_l_map.get(&iv).copied().unwrap_or("")
                    } else if let Some(inner) = cat_l_map.get(k_str) {
                        inner.get(&iv).copied().unwrap_or("")
                    } else {
                        v
                    }
                }))
                .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
            }
            wtr.flush()
                .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        }

        // 2. _copy_intermediate_files -> {module}-symptom.csv
        let symp_src = intermediate_dir.join(format!("{}-symptom.csv", module));
        if symp_src.exists() {
            fs::copy(&symp_src, f4_inter.join(format!("{}-symptom.csv", module)))
                .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        }

        // 3. _copy_likelihood_files -> {module}-likelihoods.csv in intermediate-files
        let like_csv_src = working_dir.join(format!("{}-likelihoods.csv", module));
        if like_csv_src.exists() {
            fs::copy(
                &like_csv_src,
                f4_inter.join(format!("{}-likelihoods.csv", module)),
            )
            .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        }

        // 4. _write_endorsement_rates
        let pred_file = working_dir.join(format!("{}-predictions.csv", module));
        if symp_src.exists() && pred_file.exists() {
            let (s_headers, s_rows) = read_csv_matrix(&symp_src)?;
            let (_, p_rows) = read_csv_matrix(&pred_file)?;
            let cfg = get_tariff_config(module);
            let symp_desc: HashMap<&str, &str> =
                cfg.symptom_descriptions.iter().copied().collect();

            let symptoms: Vec<String> = s_headers
                .into_iter()
                .filter(|h| {
                    let b = h.as_bytes();
                    b.len() >= 2 && b[0] == b's' && b[1..].iter().all(|c| c.is_ascii_digit())
                })
                .collect();

            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            let mut endorsements: HashMap<String, HashMap<String, usize>> = HashMap::new();
            for (s_row, p_row) in s_rows.iter().zip(p_rows.iter()) {
                let cause = p_row.get("cause34").cloned().unwrap_or_default();
                *counts.entry(cause.clone()).or_insert(0) += 1;
                let c_end = endorsements.entry(cause).or_default();
                for symp in &symptoms {
                    if safe_float(s_row.get(symp).map(|s| s.as_str())) != 0.0 {
                        *c_end.entry(symp.clone()).or_insert(0) += 1;
                    }
                }
            }

            let mut drop: HashSet<&str> = HashSet::new();
            if short_form {
                drop.extend(cfg.short_form_drop_list.iter().copied());
            }
            if !hce {
                drop.extend(cfg.hce_drop_list.iter().copied());
            }
            let causes_sorted: Vec<String> = counts.keys().cloned().collect();
            let mut er_rows = Vec::new();
            let mut hdr = vec!["symptom".to_string(), "description".to_string()];
            hdr.extend(causes_sorted.iter().cloned());
            er_rows.push(hdr);

            for symp in &symptoms {
                if drop.contains(symp.as_str()) || (!free_text && symp.starts_with("s9999")) {
                    continue;
                }
                let mut r = Vec::with_capacity(causes_sorted.len() + 2);
                r.push(symp.clone());
                r.push(symp_desc.get(symp.as_str()).copied().unwrap_or("").to_string());
                for cause in &causes_sorted {
                    let total = counts[cause] as f64;
                    let cnt = endorsements
                        .get(cause)
                        .and_then(|m| m.get(symp).copied())
                        .unwrap_or(0) as f64;
                    let rate = if total > 0.0 { cnt / total } else { 0.0 };
                    r.push(format!("{}%", round_py(rate * 100.0, 1)));
                }
                er_rows.push(r);
            }
            write_csv_rows(
                &f4_inter.join(format!("{}-endorsement-rates.csv", module)),
                &er_rows,
            )?;
        }
    }

    // _write_age_group_tabulation -> VA-data-age-groupings.csv
    if total_preds > 0 {
        let mut counts: BTreeMap<(String, String), (usize, usize, usize)> = BTreeMap::new();
        for module in MODULES {
            for row in &predictions_by_mod[module] {
                let sex = safe_int(row.get("sex").map(|s| s.as_str()));
                let age_grp =
                    get_five_year_age_group(row.get("age").map(|s| s.as_str()).unwrap_or(""));
                let cause = row.get("cause34").cloned().unwrap_or_default();
                let entry = counts.entry((age_grp, cause)).or_insert((0, 0, 0));
                entry.0 += 1;
                if sex == 1 {
                    entry.1 += 1;
                } else if sex == 2 {
                    entry.2 += 1;
                }
            }
        }
        let mut table = vec![vec![
            "cause34".to_string(),
            "icd10".to_string(),
            "age group".to_string(),
            "count".to_string(),
            "male (count)".to_string(),
            "female (count)".to_string(),
        ]];
        for ((age_grp, cause), (both_c, male_c, female_c)) in counts {
            let icd = icds::get_any_icd(&cause).unwrap_or("").to_string();
            table.push(vec![
                cause,
                icd,
                age_grp,
                both_c.to_string(),
                male_c.to_string(),
                female_c.to_string(),
            ]);
        }
        write_csv_rows(&f4.join("VA-data-age-groupings.csv"), &table)?;
    }

    // _write_report -> 4-monitoring-and-quality/report.txt
    {
        let mut f = File::create(f4.join("report.txt"))
            .map_err(|e| SmartVaError::IoError(e.to_string()))?;
        for line in &loggers.report_lines {
            writeln!(f, "{}", line).map_err(|e| SmartVaError::IoError(e.to_string()))?;
        }
        writeln!(f).map_err(|e| SmartVaError::IoError(e.to_string()))?;
        writeln!(f, "Quality summary:").map_err(|e| SmartVaError::IoError(e.to_string()))?;
        for (items, desc) in [
            (&loggers.sids, "row(s) have duplicate or missing sids"),
            (&loggers.refused, "row(s) declined the interview."),
            (
                &loggers.valid_consent,
                "row(s) did not have a valid value for consent.",
            ),
            (
                &loggers.valid_age,
                "row(s) did not have valid age data and could not be analyzed.",
            ),
            (
                &loggers.prediction,
                "row(s) had multiple causes predicted with equal likelihood.",
            ),
        ] {
            if !items.is_empty() {
                writeln!(f, "{} {}", items.len(), desc)
                    .map_err(|e| SmartVaError::IoError(e.to_string()))?;
            }
        }
        writeln!(f).map_err(|e| SmartVaError::IoError(e.to_string()))?;
        writeln!(f, "Details:").map_err(|e| SmartVaError::IoError(e.to_string()))?;
        for items in [
            &loggers.sids,
            &loggers.refused,
            &loggers.valid_consent,
            &loggers.valid_age,
            &loggers.prediction,
        ] {
            for line in items {
                writeln!(f, "{}", line).map_err(|e| SmartVaError::IoError(e.to_string()))?;
            }
        }
    }

    // Write embedded interpretation sheet directly from compiled binary
    let docx_name = "SmartVA Analyze Output Interpretation Sheet.docx";
    let _ = fs::write(f4.join(docx_name), data::EMBEDDED_INTERPRETATION_DOCX);

    // clean_up temporary root files and intermediate_dir
    let _ = fs::remove_dir_all(intermediate_dir);
    let _ = fs::remove_dir_all(working_dir.join("figures"));
    let _ = fs::remove_file(working_dir.join("warnings.txt"));
    for module in MODULES {
        for stub in [
            "csmf.csv",
            "likelihoods.csv",
            "likelihoods.xlsx",
            "predictions.csv",
            "male-csmf.csv",
            "female-csmf.csv",
        ] {
            let _ = fs::remove_file(working_dir.join(format!("{}-{}", module, stub)));
        }
    }

    Ok(())
}

/// Normalize a country string (e.g., `"IND"`, `"India"`, `"India (IND)"`) to its 3-letter ISO3 code.
pub fn resolve_country_iso3(country: &str) -> Result<Option<String>, SmartVaError> {
    let raw = country.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("Unknown") || raw.eq_ignore_ascii_case("UNK") {
        return Ok(None);
    }
    let upper = raw.to_ascii_uppercase();
    for &entry in data::countries::COUNTRIES {
        if entry == "Unknown" {
            continue;
        }
        if entry.eq_ignore_ascii_case(raw) {
            if let Some(code) = entry.split('(').nth(1).and_then(|s| s.strip_suffix(')')) {
                return Ok(Some(code.to_string()));
            }
        }
        if let Some(code) = entry.split('(').nth(1).and_then(|s| s.strip_suffix(')')) {
            if code == upper {
                return Ok(Some(code.to_string()));
            }
        }
        if let Some(name_part) = entry.split(" (").next() {
            if name_part.eq_ignore_ascii_case(raw) {
                if let Some(code) = entry.split('(').nth(1).and_then(|s| s.strip_suffix(')')) {
                    return Ok(Some(code.to_string()));
                }
            }
        }
    }
    Err(SmartVaError::ExecutionError(format!(
        "Invalid country code specified: {}",
        raw
    )))
}

/// Pre-warm all 3 age-group tariff & validation models (`adult`, `child`, `neonate`) in memory.
pub fn warm_all_models() -> Result<(), SmartVaError> {
    for module in MODULES {
        let _ = get_or_build_trained_model(module, true, true, false)?;
    }
    Ok(())
}

fn build_endorsement_rates_rows(
    module: &str,
    s_headers: &[String],
    s_rows: &[Row],
    pred_causes: &[String],
    short_form: bool,
    free_text: bool,
    hce: bool,
) -> Vec<Vec<String>> {
    let cfg = get_tariff_config(module);
    let symp_desc: HashMap<&str, &str> = cfg.symptom_descriptions.iter().copied().collect();

    let symptoms: Vec<String> = s_headers
        .iter()
        .filter(|h| {
            let b = h.as_bytes();
            b.len() >= 2 && b[0] == b's' && b[1..].iter().all(|c| c.is_ascii_digit())
        })
        .cloned()
        .collect();

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut endorsements: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for (s_row, cause) in s_rows.iter().zip(pred_causes.iter()) {
        *counts.entry(cause.clone()).or_insert(0) += 1;
        let c_end = endorsements.entry(cause.clone()).or_default();
        for symp in &symptoms {
            if safe_float(s_row.get(symp).map(|s| s.as_str())) != 0.0 {
                *c_end.entry(symp.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut drop: HashSet<&str> = HashSet::new();
    if short_form {
        drop.extend(cfg.short_form_drop_list.iter().copied());
    }
    if !hce {
        drop.extend(cfg.hce_drop_list.iter().copied());
    }
    let causes_sorted: Vec<String> = counts.keys().cloned().collect();
    let mut er_rows = Vec::new();
    let mut hdr = vec!["symptom".to_string(), "description".to_string()];
    hdr.extend(causes_sorted.iter().cloned());
    er_rows.push(hdr);

    for symp in &symptoms {
        if drop.contains(symp.as_str()) || (!free_text && symp.starts_with("s9999")) {
            continue;
        }
        let mut r = Vec::with_capacity(causes_sorted.len() + 2);
        r.push(symp.clone());
        r.push(symp_desc.get(symp.as_str()).copied().unwrap_or("").to_string());
        for cause in &causes_sorted {
            let total = counts[cause] as f64;
            let cnt = endorsements
                .get(cause)
                .and_then(|m| m.get(symp).copied())
                .unwrap_or(0) as f64;
            let rate = if total > 0.0 { cnt / total } else { 0.0 };
            r.push(format!("{}%", round_py(rate * 100.0, 1)));
        }
        er_rows.push(r);
    }
    er_rows
}

fn build_f1_headers() -> Vec<String> {
    [
        "sid",
        "national_id",
        "name",
        "name2",
        "surname",
        "surname2",
        "geography1",
        "geography2",
        "geography3",
        "geography4",
        "geography5",
        "cause34",
        "cause list #",
        "icd10",
        "age",
        "sex",
        "birth_date",
        "death_date",
        "interview_date",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn build_f1_row_from_prediction(p: &SmartVaPrediction) -> Vec<String> {
    vec![
        p.sid.clone(),
        p.national_id.clone(),
        p.name.clone(),
        p.name2.clone(),
        p.surname.clone(),
        p.surname2.clone(),
        p.geography1.clone(),
        p.geography2.clone(),
        p.geography3.clone(),
        p.geography4.clone(),
        p.geography5.clone(),
        p.cause34.clone(),
        p.cause_list_number.clone(),
        p.icd10.clone(),
        p.age.map(py_float_str).unwrap_or_default(),
        p.sex.map(|s| s.to_string()).unwrap_or_default(),
        p.birth_date.clone(),
        p.death_date.clone(),
        p.interview_date.clone(),
    ]
}

fn read_csv_raw_rows(path: &Path) -> Option<Vec<Vec<String>>> {
    if !path.exists() {
        return None;
    }
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .ok()?;
    let mut out = Vec::new();
    for rec in rdr.records().flatten() {
        out.push(rec.iter().map(|s| s.to_string()).collect());
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[derive(Default, Clone)]
struct PreppedDemographics {
    national_id: String,
    name: String,
    name2: String,
    surname: String,
    surname2: String,
    geography1: String,
    geography2: String,
    geography3: String,
    geography4: String,
    geography5: String,
    birth_date: String,
    death_date: String,
    interview_date: String,
}

impl PreppedDemographics {
    fn from_row(raw_row: &Row) -> Self {
        let mut interview_date = raw_row
            .get("interviewdate")
            .cloned()
            .unwrap_or_default();
        if is_invalid_interview_date(&interview_date) {
            interview_date.clear();
        }
        Self {
            national_id: raw_row.get("gen_6_7").cloned().unwrap_or_default(),
            name: raw_row.get("gen_5_0").cloned().unwrap_or_default(),
            name2: raw_row.get("gen_5_0c").cloned().unwrap_or_default(),
            surname: raw_row.get("gen_5_0a").cloned().unwrap_or_default(),
            surname2: raw_row.get("gen_5_0a2").cloned().unwrap_or_default(),
            geography1: raw_row.get("gen_2_3a").cloned().unwrap_or_default(),
            geography2: raw_row.get("gen_2_3b").cloned().unwrap_or_default(),
            geography3: raw_row.get("gen_2_3c").cloned().unwrap_or_default(),
            geography4: raw_row.get("gen_2_3d").cloned().unwrap_or_default(),
            geography5: raw_row.get("gen_2_3e").cloned().unwrap_or_default(),
            birth_date: make_output_date(
                raw_row.get("gen_5_1a").map(|s| s.as_str()).unwrap_or(""),
                raw_row.get("gen_5_1b").map(|s| s.as_str()).unwrap_or(""),
                raw_row.get("gen_5_1c").map(|s| s.as_str()).unwrap_or(""),
            ),
            death_date: make_output_date(
                raw_row.get("gen_5_3a").map(|s| s.as_str()).unwrap_or(""),
                raw_row.get("gen_5_3b").map(|s| s.as_str()).unwrap_or(""),
                raw_row.get("gen_5_3c").map(|s| s.as_str()).unwrap_or(""),
            ),
            interview_date,
        }
    }

    fn apply_to(&self, pred: &mut SmartVaPrediction) {
        pred.national_id = self.national_id.clone();
        pred.name = self.name.clone();
        pred.name2 = self.name2.clone();
        pred.surname = self.surname.clone();
        pred.surname2 = self.surname2.clone();
        pred.geography1 = self.geography1.clone();
        pred.geography2 = self.geography2.clone();
        pred.geography3 = self.geography3.clone();
        pred.geography4 = self.geography4.clone();
        pred.geography5 = self.geography5.clone();
        pred.birth_date = self.birth_date.clone();
        pred.death_date = self.death_date.clone();
        pred.interview_date = self.interview_date.clone();
    }
}

fn enrich_predictions_from_prepped_csv(
    prepped_file: &Path,
    preds: &mut [SmartVaPrediction],
) -> Result<(), SmartVaError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(prepped_file)
        .map_err(|e| SmartVaError::IoError(e.to_string()))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let idx = |name: &str| headers.iter().position(|h| h == name);
    let i_interview = idx("interviewdate");
    let i_nat = idx("gen_6_7");
    let i_name = idx("gen_5_0");
    let i_name2 = idx("gen_5_0c");
    let i_sur = idx("gen_5_0a");
    let i_sur2 = idx("gen_5_0a2");
    let i_g1 = idx("gen_2_3a");
    let i_g2 = idx("gen_2_3b");
    let i_g3 = idx("gen_2_3c");
    let i_g4 = idx("gen_2_3d");
    let i_g5 = idx("gen_2_3e");
    let i_b1 = idx("gen_5_1a");
    let i_b2 = idx("gen_5_1b");
    let i_b3 = idx("gen_5_1c");
    let i_d1 = idx("gen_5_3a");
    let i_d2 = idx("gen_5_3b");
    let i_d3 = idx("gen_5_3c");

    let get_cell = |rec: &csv::StringRecord, opt_i: Option<usize>| -> String {
        opt_i
            .and_then(|i| rec.get(i))
            .unwrap_or("")
            .to_string()
    };

    for (pred, rec_res) in preds.iter_mut().zip(rdr.records()) {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        let mut interview_date = get_cell(&rec, i_interview);
        if is_invalid_interview_date(&interview_date) {
            interview_date.clear();
        }
        pred.national_id = get_cell(&rec, i_nat);
        pred.name = get_cell(&rec, i_name);
        pred.name2 = get_cell(&rec, i_name2);
        pred.surname = get_cell(&rec, i_sur);
        pred.surname2 = get_cell(&rec, i_sur2);
        pred.geography1 = get_cell(&rec, i_g1);
        pred.geography2 = get_cell(&rec, i_g2);
        pred.geography3 = get_cell(&rec, i_g3);
        pred.geography4 = get_cell(&rec, i_g4);
        pred.geography5 = get_cell(&rec, i_g5);
        pred.birth_date = make_output_date(
            &get_cell(&rec, i_b1),
            &get_cell(&rec, i_b2),
            &get_cell(&rec, i_b3),
        );
        pred.death_date = make_output_date(
            &get_cell(&rec, i_d1),
            &get_cell(&rec, i_d2),
            &get_cell(&rec, i_d3),
        );
        pred.interview_date = interview_date;
    }
    Ok(())
}

fn format_report_text(loggers: &ReportLoggers) -> String {
    let mut out = String::new();
    for line in &loggers.report_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str("Quality summary:\n");
    for (items, desc) in [
        (&loggers.sids, "row(s) have duplicate or missing sids"),
        (&loggers.refused, "row(s) declined the interview."),
        (
            &loggers.valid_consent,
            "row(s) did not have a valid value for consent.",
        ),
        (
            &loggers.valid_age,
            "row(s) did not have valid age data and could not be analyzed.",
        ),
        (
            &loggers.prediction,
            "row(s) had multiple causes predicted with equal likelihood.",
        ),
    ] {
        if !items.is_empty() {
            out.push_str(&format!("{} {}\n", items.len(), desc));
        }
    }
    out.push('\n');
    out.push_str("Details:\n");
    for items in [
        &loggers.sids,
        &loggers.refused,
        &loggers.valid_consent,
        &loggers.valid_age,
        &loggers.prediction,
    ] {
        for line in items {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Execute the full SmartVA-Analyze pipeline in pure Rust, writing the complete 4-folder
/// directory tree (`1-individual-cause-of-death/`, `2-csmf/`, `3-graphs-and-tables/`,
/// `4-monitoring-and-quality/`) to `output_dir`.
pub fn run_smartva_pipeline_directory(
    input_csv: &Path,
    output_dir: &Path,
    options: &SmartVaRunOptions,
) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
    if !input_csv.exists() {
        return Err(SmartVaError::IoError(format!(
            "Input CSV file does not exist: {}",
            input_csv.display()
        )));
    }
    if !options.write_csv && !options.figures {
        return run_smartva_csv_in_memory(input_csv, options);
    }
    let iso3 = resolve_country_iso3(&options.country)?;
    fs::create_dir_all(output_dir).map_err(|e| SmartVaError::IoError(e.to_string()))?;
    let intermediate_dir = output_dir.join("intermediate-files");
    fs::create_dir_all(&intermediate_dir).map_err(|e| SmartVaError::IoError(e.to_string()))?;

    let mut loggers = ReportLoggers::default();
    let cleanheaders_path = intermediate_dir.join("cleanheaders.csv");
    let headers = format_headers_step(input_csv, &cleanheaders_path, &mut loggers)?;

    loggers.report_lines.push("Analysis parameters:".to_string());
    loggers
        .report_lines
        .push(format!("- Input file: {}", input_csv.display()));
    loggers
        .report_lines
        .push(format!("- Output folder: {}", output_dir.display()));
    loggers.report_lines.push(format!(
        "- Country: {}",
        iso3.as_deref().unwrap_or("None")
    ));
    loggers
        .report_lines
        .push(format!("- HIV Region: {}", options.hiv));
    loggers
        .report_lines
        .push(format!("- Malaria Region: {}", options.malaria));
    loggers.report_lines.push(String::new());

    let who_questionnaire = who_questionnaire_test(&headers);
    let (short_form, who_2016, form_name) = if who_questionnaire {
        let is_2016 = headers.iter().any(|h| h == "Id10219");
        let name = if is_2016 {
            "WHO 2016 Questionnaire"
        } else {
            "WHO 2022 Questionnaire"
        };
        (true, is_2016, name)
    } else {
        let is_short = headers.iter().any(|h| h == "adult_7_11");
        let name = if is_short {
            "PHMRC Shortened Questionnaire"
        } else {
            "PHMRC Full Questionnaire"
        };
        (is_short, false, name)
    };
    loggers
        .report_lines
        .push(format!("Detected {}", form_name));

    if who_questionnaire {
        run_who_prep(&cleanheaders_path, who_2016)?;
    }

    let (adult_data, child_data, neonate_data) =
        run_common_prep(&intermediate_dir, short_form, &mut loggers)?;

    let mut predictions = Vec::new();
    let mut csv_tables: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for (module, has_data) in [
        ("adult", adult_data),
        ("child", child_data),
        ("neonate", neonate_data),
    ] {
        if has_data {
            let prepped_file = intermediate_dir.join(format!("{}-prepped.csv", module));
            run_pre_symptom_prep(&intermediate_dir, module, short_form, &mut loggers)?;
            run_rules_prep(&intermediate_dir, module)?;
            run_symptom_prep(&intermediate_dir, module, &mut loggers)?;
            let mut mod_preds = run_tariff_prep(
                output_dir,
                &intermediate_dir,
                module,
                short_form,
                options,
                iso3.as_deref(),
                &mut loggers,
            )?;
            enrich_predictions_from_prepped_csv(&prepped_file, &mut mod_preds)?;
            for stub in ["tariff-scores.csv", "tariff-ranks.csv", "external-ranks.csv"] {
                let name = format!("{}-{}", module, stub);
                if let Some(rows) = read_csv_raw_rows(&intermediate_dir.join(&name)) {
                    csv_tables.insert(name, rows);
                }
            }
            predictions.extend(mod_preds);
        }
    }

    run_output_prep(
        output_dir,
        &intermediate_dir,
        short_form,
        options.freetext,
        options.hce,
        &loggers,
    )?;

    let f1 = output_dir.join("1-individual-cause-of-death");
    let f2 = output_dir.join("2-csmf");
    let f3 = output_dir.join("3-graphs-and-tables");
    let f4 = output_dir.join("4-monitoring-and-quality");
    let f4_inter = f4.join("intermediate-files");
    for (key, path) in [
        (
            "individual-cause-of-death.csv".to_string(),
            f1.join("individual-cause-of-death.csv"),
        ),
        ("csmf.csv".to_string(), f2.join("csmf.csv")),
        (
            "gbd-level1-csmf.csv".to_string(),
            f2.join("gbd-level1-csmf.csv"),
        ),
        (
            "causes-of-death.csv".to_string(),
            f3.join("causes-of-death.csv"),
        ),
        (
            "VA-data-age-groupings.csv".to_string(),
            f4.join("VA-data-age-groupings.csv"),
        ),
    ] {
        if let Some(rows) = read_csv_raw_rows(&path) {
            csv_tables.insert(key, rows);
        }
    }
    for module in MODULES {
        for (key, path) in [
            (
                format!("{}-predictions.csv", module),
                f1.join(format!("{}-predictions.csv", module)),
            ),
            (
                format!("{}-csmf.csv", module),
                f2.join(format!("{}-csmf.csv", module)),
            ),
            (
                format!("{}-likelihoods.csv", module),
                f4_inter.join(format!("{}-likelihoods.csv", module)),
            ),
            (
                format!("{}-endorsement-rates.csv", module),
                f4_inter.join(format!("{}-endorsement-rates.csv", module)),
            ),
        ] {
            if let Some(rows) = read_csv_raw_rows(&path) {
                csv_tables.insert(key, rows);
            }
        }
    }

    let xlsx_files_written = if options.write_csv {
        xlsx::generate_likelihood_xlsx_files(output_dir)
            .map_err(|e| SmartVaError::ExecutionError(e.to_string()))?
    } else {
        0
    };
    let png_charts_written = charts::generate_all_smartva_charts(output_dir, options.figures)
        .map_err(|e| SmartVaError::ExecutionError(e.to_string()))?;

    let read_csmf_map = |p: &Path| -> BTreeMap<String, f64> {
        let mut m = BTreeMap::new();
        if let Ok((_, rows)) = read_csv_matrix(p) {
            for r in rows {
                if let (Some(c), Some(v)) = (r.get("cause34"), r.get("all")) {
                    if let Ok(fv) = v.parse::<f64>() {
                        m.insert(c.clone(), fv);
                    }
                }
            }
        }
        m
    };
    let read_csmf_rows = |p: &Path| -> Vec<SmartVaCsmfRow> {
        let mut out = Vec::new();
        if let Ok((_, rows)) = read_csv_matrix(p) {
            for r in rows {
                out.push(SmartVaCsmfRow {
                    cause34: r.get("cause34").cloned().unwrap_or_default(),
                    cause_list_number: r.get("cause list #").cloned(),
                    icd10: r.get("icd10").cloned().unwrap_or_default(),
                    all: r.get("all").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    male: r.get("male").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                    female: r.get("female").and_then(|s| s.parse().ok()).unwrap_or(0.0),
                });
            }
        }
        out
    };

    let mut gbd_level1_csmf_rows = Vec::new();
    if let Ok((_, rows)) = read_csv_matrix(&f2.join("gbd-level1-csmf.csv")) {
        for r in rows {
            gbd_level1_csmf_rows.push(SmartVaGbdCsmfRow {
                cause: r.get("cause").cloned().unwrap_or_default(),
                cause_name: r.get("cause_name").cloned().unwrap_or_default(),
                csmf: r.get("CSMF").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            });
        }
    }

    let mut causes_of_death_rows = Vec::new();
    if let Ok((_, rows)) = read_csv_matrix(&f3.join("causes-of-death.csv")) {
        for r in rows {
            causes_of_death_rows.push(SmartVaCausesOfDeathRow {
                module: r.get("module").cloned().unwrap_or_default(),
                cause34: r.get("cause34").cloned().unwrap_or_default(),
                age: r.get("age").cloned().unwrap_or_default(),
                sex: r.get("sex").cloned().unwrap_or_default(),
                counts: r.get("counts").and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
    }

    let mut age_groupings_rows = Vec::new();
    if let Ok((_, rows)) = read_csv_matrix(&f4.join("VA-data-age-groupings.csv")) {
        for r in rows {
            age_groupings_rows.push(SmartVaAgeGroupingRow {
                cause34: r.get("cause34").cloned().unwrap_or_default(),
                icd10: r.get("icd10").cloned().unwrap_or_default(),
                age_group: r.get("age group").cloned().unwrap_or_default(),
                count: r.get("count").and_then(|s| s.parse().ok()).unwrap_or(0),
                male_count: r
                    .get("male (count)")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
                female_count: r
                    .get("female (count)")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            });
        }
    }

    let csmf_all = read_csmf_map(&f2.join("csmf.csv"));
    let csmf_adult = read_csmf_map(&f2.join("adult-csmf.csv"));
    let csmf_child = read_csmf_map(&f2.join("child-csmf.csv"));
    let csmf_neonate = read_csmf_map(&f2.join("neonate-csmf.csv"));
    let csmf_rows = read_csmf_rows(&f2.join("csmf.csv"));
    let csmf_adult_rows = read_csmf_rows(&f2.join("adult-csmf.csv"));
    let csmf_child_rows = read_csmf_rows(&f2.join("child-csmf.csv"));
    let csmf_neonate_rows = read_csmf_rows(&f2.join("neonate-csmf.csv"));

    if !options.write_csv {
        // User requested charts only (--csv false --charts true): remove CSV folders except 3-graphs-and-tables/*.png
        let _ = fs::remove_dir_all(output_dir.join("1-individual-cause-of-death"));
        let _ = fs::remove_dir_all(&f2);
        let _ = fs::remove_dir_all(&f4);
        let _ = fs::remove_file(f3.join("causes-of-death.csv"));
    }

    Ok(SmartVaCliDirectoryResult {
        total_vas: predictions.len(),
        predictions,
        csv_tables,
        csmf_all,
        csmf_adult,
        csmf_child,
        csmf_neonate,
        csmf_rows,
        csmf_adult_rows,
        csmf_child_rows,
        csmf_neonate_rows,
        gbd_level1_csmf_rows,
        causes_of_death_rows,
        age_groupings_rows,
        warnings: loggers.warnings.clone(),
        report_text: format_report_text(&loggers),
        xlsx_files_written,
        png_charts_written,
    })
}

/// Execute the SmartVA-Analyze pipeline 100% in memory on `raw_headers` and `raw_rows`
/// without writing any CSV files, `.xlsx` workbooks, or `.png` charts to disk.
pub fn run_smartva_in_memory(
    raw_headers: &[String],
    raw_rows: &[Row],
    options: &SmartVaRunOptions,
) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
    if raw_headers.is_empty() || raw_rows.is_empty() {
        return Err(SmartVaError::ExecutionError(
            "Input data does not contain any rows.".to_string(),
        ));
    }
    let iso3 = resolve_country_iso3(&options.country)?;
    let mut loggers = ReportLoggers::default();

    let mut headers: Vec<String> = raw_headers.iter().map(|h| format_header_name(h)).collect();
    let mut matrix: Vec<Row> = Vec::with_capacity(raw_rows.len());
    for raw_row in raw_rows {
        let mut row = Row::with_capacity(headers.len());
        for (raw_h, clean_h) in raw_headers.iter().zip(headers.iter()) {
            let val = raw_row
                .get(raw_h)
                .or_else(|| raw_row.get(clean_h))
                .cloned()
                .unwrap_or_default();
            row.insert(clean_h.clone(), val);
        }
        matrix.push(row);
    }

    let who_questionnaire = who_questionnaire_test(&headers);
    let (short_form, who_2016, form_name) = if who_questionnaire {
        let is_2016 = headers.iter().any(|h| h == "Id10219");
        let name = if is_2016 {
            "WHO 2016 Questionnaire"
        } else {
            "WHO 2022 Questionnaire"
        };
        (true, is_2016, name)
    } else {
        let is_short = headers.iter().any(|h| h == "adult_7_11");
        let name = if is_short {
            "PHMRC Shortened Questionnaire"
        } else {
            "PHMRC Full Questionnaire"
        };
        (is_short, false, name)
    };
    loggers.report_lines.push("Analysis parameters:".to_string());
    loggers.report_lines.push(format!(
        "- Country: {}",
        iso3.as_deref().unwrap_or("None")
    ));
    loggers
        .report_lines
        .push(format!("- HIV Region: {}", options.hiv));
    loggers
        .report_lines
        .push(format!("- Malaria Region: {}", options.malaria));
    loggers.report_lines.push(String::new());
    loggers
        .report_lines
        .push(format!("Detected {}", form_name));

    if who_questionnaire {
        headers = who_prep_in_memory(&mut matrix, who_2016);
    }

    let (common_headers, mut matrix_data) =
        common_prep_in_memory(headers, matrix, short_form, &mut loggers);

    let mut predictions = Vec::new();
    let mut csv_tables: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    let f1_headers = build_f1_headers();
    let mut all_f1_rows = vec![f1_headers.clone()];
    let mut preds_by_mod: HashMap<&'static str, Vec<SmartVaPrediction>> = HashMap::new();
    let mut mod_csmfs: HashMap<&'static str, HashMap<&'static str, BTreeMap<String, f64>>> =
        HashMap::new();
    let mut csmf_adult = BTreeMap::new();
    let mut csmf_child = BTreeMap::new();
    let mut csmf_neonate = BTreeMap::new();
    let mut csmf_adult_rows = Vec::new();
    let mut csmf_child_rows = Vec::new();
    let mut csmf_neonate_rows = Vec::new();

    for module in MODULES {
        let mod_rows = matrix_data.remove(module).unwrap_or_default();
        if mod_rows.is_empty() {
            preds_by_mod.insert(module, Vec::new());
            mod_csmfs.insert(
                module,
                HashMap::from([
                    ("both", BTreeMap::new()),
                    ("male", BTreeMap::new()),
                    ("female", BTreeMap::new()),
                ]),
            );
            csv_tables.insert(
                format!("{}-csmf.csv", module),
                vec![vec![
                    "cause34".to_string(),
                    "cause list #".to_string(),
                    "icd10".to_string(),
                    "all".to_string(),
                    "male".to_string(),
                    "female".to_string(),
                ]],
            );
            continue;
        }
        let prepped_demog: Vec<PreppedDemographics> =
            mod_rows.iter().map(PreppedDemographics::from_row).collect();
        let (ps_headers, ps_matrix) = pre_symptom_prep_in_memory(
            module,
            common_headers.clone(),
            mod_rows,
            short_form,
            &mut loggers,
        );
        let (rules_headers, rules_matrix) = rules_prep_in_memory(module, ps_headers, ps_matrix);
        let (symptom_headers, symptom_matrix) =
            symptom_prep_in_memory(module, rules_headers, rules_matrix, &mut loggers);
        let mut res = tariff_prep_in_memory(
            module,
            &symptom_matrix,
            short_form,
            options,
            iso3.as_deref(),
            &mut loggers,
        )?;
        let mut mod_f1_rows = vec![f1_headers.clone()];
        for (pred, demog) in res.predictions.iter_mut().zip(prepped_demog.iter()) {
            demog.apply_to(pred);
            let f1_r = build_f1_row_from_prediction(pred);
            all_f1_rows.push(f1_r.clone());
            mod_f1_rows.push(f1_r);
        }
        csv_tables.insert(format!("{}-predictions.csv", module), mod_f1_rows);
        csv_tables.insert(format!("{}-likelihoods.csv", module), res.mp_rows.clone());
        csv_tables.insert(
            format!("{}-tariff-scores.csv", module),
            build_tariff_scores_rows(&res.model.cause_list, &res.user_data),
        );
        csv_tables.insert(
            format!("{}-tariff-ranks.csv", module),
            build_tariff_ranks_rows(&res.model.cause_list, &res.user_data, false),
        );
        csv_tables.insert(
            format!("{}-external-ranks.csv", module),
            build_tariff_ranks_rows(&res.model.cause_list, &res.user_data, true),
        );
        let pred_causes: Vec<String> = res.predictions.iter().map(|p| p.cause34.clone()).collect();
        csv_tables.insert(
            format!("{}-endorsement-rates.csv", module),
            build_endorsement_rates_rows(
                module,
                &symptom_headers,
                &symptom_matrix,
                &pred_causes,
                short_form,
                options.freetext,
                options.hce,
            ),
        );
        preds_by_mod.insert(module, res.predictions.clone());
        predictions.extend(res.predictions);

        let normalize_counts = |counts: &BTreeMap<String, f64>| -> BTreeMap<String, f64> {
            let total: f64 = counts.values().sum();
            let mut m = BTreeMap::new();
            if total > 0.0 {
                for (k, &v) in counts {
                    m.insert(k.clone(), v / total);
                }
            }
            m
        };
        let both_map = normalize_counts(&res.cause_counts);
        let male_map = normalize_counts(&res.counts_by_sex[&1]);
        let female_map = normalize_counts(&res.counts_by_sex[&2]);

        let cfg = get_tariff_config(module);
        let cause_num_by_name: HashMap<&str, i32> =
            cfg.causes.iter().map(|&(num, name)| (name, num)).collect();
        let mut rounded_both = BTreeMap::new();
        let mut mod_table_rows = Vec::new();
        let mut mod_csmf_csv_rows = vec![vec![
            "cause34".to_string(),
            "cause list #".to_string(),
            "icd10".to_string(),
            "all".to_string(),
            "male".to_string(),
            "female".to_string(),
        ]];
        for cause in both_map.keys() {
            let c_num = cause_num_by_name
                .get(cause.as_str())
                .map(|n| n.to_string())
                .unwrap_or_default();
            let icd = icds::get_icd(module, cause).unwrap_or("").to_string();
            let all_s = both_map
                .get(cause)
                .map(|&v| round_py(v, 3))
                .unwrap_or_else(|| "0".to_string());
            let male_s = male_map
                .get(cause)
                .map(|&v| round_py(v, 3))
                .unwrap_or_else(|| "0".to_string());
            let female_s = female_map
                .get(cause)
                .map(|&v| round_py(v, 3))
                .unwrap_or_else(|| "0".to_string());
            let all_v = all_s.parse::<f64>().unwrap_or(0.0);
            let male_v = male_s.parse::<f64>().unwrap_or(0.0);
            let female_v = female_s.parse::<f64>().unwrap_or(0.0);
            rounded_both.insert(cause.clone(), all_v);
            mod_csmf_csv_rows.push(vec![
                cause.clone(),
                c_num.clone(),
                icd.clone(),
                all_s,
                male_s,
                female_s,
            ]);
            mod_table_rows.push(SmartVaCsmfRow {
                cause34: cause.clone(),
                cause_list_number: Some(c_num),
                icd10: icd,
                all: all_v,
                male: male_v,
                female: female_v,
            });
        }
        csv_tables.insert(format!("{}-csmf.csv", module), mod_csmf_csv_rows);
        mod_csmfs.insert(
            module,
            HashMap::from([("both", both_map), ("male", male_map), ("female", female_map)]),
        );
        match module {
            "adult" => {
                csmf_adult = rounded_both;
                csmf_adult_rows = mod_table_rows;
            }
            "child" => {
                csmf_child = rounded_both;
                csmf_child_rows = mod_table_rows;
            }
            "neonate" => {
                csmf_neonate = rounded_both;
                csmf_neonate_rows = mod_table_rows;
            }
            _ => {}
        }
    }
    csv_tables.insert("individual-cause-of-death.csv".to_string(), all_f1_rows);

    let mut csmf_all = BTreeMap::new();
    let mut csmf_rows = Vec::new();
    let mut gbd_level1_csmf_rows = Vec::new();
    let mut causes_of_death_rows = Vec::new();
    let mut age_groupings_rows = Vec::new();

    let total_preds = predictions.len();
    if total_preds > 0 {
        let total_f = total_preds as f64;
        let mut all_csmf: BTreeMap<String, HashMap<&str, f64>> = BTreeMap::new();
        for module in MODULES {
            let weight = (preds_by_mod[module].len() as f64) / total_f;
            if let Some(m_csmf) = mod_csmfs.get(module) {
                for (&sex_lbl, values) in m_csmf {
                    for (cause, &val) in values {
                        *all_csmf
                            .entry(cause.clone())
                            .or_default()
                            .entry(sex_lbl)
                            .or_insert(0.0) += val * weight;
                    }
                }
            }
        }
        let mut csmf_csv_rows = vec![vec![
            "cause34".to_string(),
            "icd10".to_string(),
            "all".to_string(),
            "male".to_string(),
            "female".to_string(),
        ]];
        for (cause, sex_vals) in &all_csmf {
            let icd = icds::get_any_icd(cause).unwrap_or("").to_string();
            let all_s = sex_vals
                .get("both")
                .map(|&v| round_py(v, 3))
                .unwrap_or_else(|| "0".to_string());
            let male_s = sex_vals
                .get("male")
                .map(|&v| round_py(v, 3))
                .unwrap_or_else(|| "0".to_string());
            let female_s = sex_vals
                .get("female")
                .map(|&v| round_py(v, 3))
                .unwrap_or_else(|| "0".to_string());
            let all_v = all_s.parse::<f64>().unwrap_or(0.0);
            let male_v = male_s.parse::<f64>().unwrap_or(0.0);
            let female_v = female_s.parse::<f64>().unwrap_or(0.0);
            csmf_all.insert(cause.clone(), all_v);
            csmf_csv_rows.push(vec![
                cause.clone(),
                icd.clone(),
                all_s,
                male_s,
                female_s,
            ]);
            csmf_rows.push(SmartVaCsmfRow {
                cause34: cause.clone(),
                cause_list_number: None,
                icd10: icd,
                all: all_v,
                male: male_v,
                female: female_v,
            });
        }
        csv_tables.insert("csmf.csv".to_string(), csmf_csv_rows);

        let mut gbd_csmf: BTreeMap<&str, f64> = BTreeMap::new();
        for module in MODULES {
            let weight = (preds_by_mod[module].len() as f64) / total_f;
            if let Some(both_vals) = mod_csmfs.get(module).and_then(|m| m.get("both")) {
                for (cause, &val) in both_vals {
                    if let Some(gbd_code) = gbd_causes::get_gbd_level1(module, cause) {
                        *gbd_csmf.entry(gbd_code).or_insert(0.0) += val * weight;
                    }
                }
            }
        }
        let mut gbd_csv_rows = vec![vec![
            "cause".to_string(),
            "cause_name".to_string(),
            "CSMF".to_string(),
        ]];
        for (code, val) in gbd_csmf {
            let name = gbd_causes::GBD_LEVEL1_CAUSE_NAMES
                .iter()
                .find(|&&(k, _)| k == code)
                .map(|&(_, v)| v)
                .unwrap_or("");
            let csmf_s = round_py(val, 3);
            let csmf_v = csmf_s.parse::<f64>().unwrap_or(0.0);
            gbd_csv_rows.push(vec![code.to_string(), name.to_string(), csmf_s]);
            gbd_level1_csmf_rows.push(SmartVaGbdCsmfRow {
                cause: code.to_string(),
                cause_name: name.to_string(),
                csmf: csmf_v,
            });
        }
        csv_tables.insert("gbd-level1-csmf.csv".to_string(), gbd_csv_rows);

        // causes-of-death.csv
        let age_groups_order: [(&str, &str); 14] = [
            ("neonate0", " 00 days"),
            ("neonate1", " 00-07 days"),
            ("neonate2", " 08-28 days"),
            ("child1", " 29 days - <1 year"),
            ("child2", "01-04 years"),
            ("child3", "05-11 years"),
            ("adult1", "12-19 years"),
            ("adult2", "20-29 years"),
            ("adult3", "30-39 years"),
            ("adult4", "40-49 years"),
            ("adult5", "50-59 years"),
            ("adult6", "60-69 years"),
            ("adult7", "70-79 years"),
            ("adult8", "80+"),
        ];
        let mut cod_counts: HashMap<(&str, &str, String), usize> = HashMap::new();
        let mut mod_causes: HashMap<&str, BTreeSet<String>> = HashMap::new();
        for module in MODULES {
            mod_causes.insert(module, BTreeSet::new());
            for pred in &preds_by_mod[module] {
                let cause = pred.cause34.clone();
                mod_causes.get_mut(module).unwrap().insert(cause.clone());
                let age_grp = if cause == "Stillbirth" {
                    " 00 days"
                } else {
                    bin_ages_for_table(module, pred.age.unwrap_or(0.0))
                };
                let sex_lbl = match pred.sex.unwrap_or(0) {
                    1 => "Male",
                    2 => "Female",
                    _ => "Missing",
                };
                *cod_counts.entry((age_grp, sex_lbl, cause)).or_insert(0) += 1;
            }
        }
        let mut cod_csv_rows = vec![vec![
            "module".to_string(),
            "cause34".to_string(),
            "age".to_string(),
            "sex".to_string(),
            "counts".to_string(),
        ]];
        for (age_i, age_grp) in age_groups_order {
            let module = &age_i[..age_i.len() - 1];
            for sex_lbl in ["Male", "Female", "Missing"] {
                if let Some(c_set) = mod_causes.get(module) {
                    for cause in c_set {
                        if let Some(&cnt) = cod_counts.get(&(age_grp, sex_lbl, cause.clone())) {
                            if cnt > 0 {
                                cod_csv_rows.push(vec![
                                    module.to_string(),
                                    cause.clone(),
                                    age_grp.to_string(),
                                    sex_lbl.to_string(),
                                    cnt.to_string(),
                                ]);
                                causes_of_death_rows.push(SmartVaCausesOfDeathRow {
                                    module: module.to_string(),
                                    cause34: cause.clone(),
                                    age: age_grp.to_string(),
                                    sex: sex_lbl.to_string(),
                                    counts: cnt,
                                });
                            }
                        }
                    }
                }
            }
        }
        csv_tables.insert("causes-of-death.csv".to_string(), cod_csv_rows);

        // VA-data-age-groupings.csv
        let mut ag_counts: BTreeMap<(String, String), (usize, usize, usize)> = BTreeMap::new();
        for module in MODULES {
            for pred in &preds_by_mod[module] {
                let sex = pred.sex.unwrap_or(0);
                let age_str = pred.age.map(py_float_str).unwrap_or_default();
                let age_grp = get_five_year_age_group(&age_str);
                let cause = pred.cause34.clone();
                let entry = ag_counts.entry((age_grp, cause)).or_insert((0, 0, 0));
                entry.0 += 1;
                if sex == 1 {
                    entry.1 += 1;
                } else if sex == 2 {
                    entry.2 += 1;
                }
            }
        }
        let mut ag_csv_rows = vec![vec![
            "cause34".to_string(),
            "icd10".to_string(),
            "age group".to_string(),
            "count".to_string(),
            "male (count)".to_string(),
            "female (count)".to_string(),
        ]];
        for ((age_grp, cause), (both_c, male_c, female_c)) in ag_counts {
            let icd = icds::get_any_icd(&cause).unwrap_or("").to_string();
            ag_csv_rows.push(vec![
                cause.clone(),
                icd.clone(),
                age_grp.clone(),
                both_c.to_string(),
                male_c.to_string(),
                female_c.to_string(),
            ]);
            age_groupings_rows.push(SmartVaAgeGroupingRow {
                cause34: cause,
                icd10: icd,
                age_group: age_grp,
                count: both_c,
                male_count: male_c,
                female_count: female_c,
            });
        }
        csv_tables.insert("VA-data-age-groupings.csv".to_string(), ag_csv_rows);
    }

    Ok(SmartVaCliDirectoryResult {
        total_vas: predictions.len(),
        predictions,
        csv_tables,
        csmf_all,
        csmf_adult,
        csmf_child,
        csmf_neonate,
        csmf_rows,
        csmf_adult_rows,
        csmf_child_rows,
        csmf_neonate_rows,
        gbd_level1_csmf_rows,
        causes_of_death_rows,
        age_groupings_rows,
        warnings: loggers.warnings.clone(),
        report_text: format_report_text(&loggers),
        xlsx_files_written: 0,
        png_charts_written: 0,
    })
}

/// Score an in-memory CSV byte buffer (`&[u8]`) 100% in memory without writing any files to disk.
pub fn run_smartva_csv_bytes_in_memory(
    csv_bytes: &[u8],
    options: &SmartVaRunOptions,
) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_bytes);
    let mut records = rdr.records();
    let Some(first_res) = records.next() else {
        return Err(SmartVaError::ExecutionError(
            "Input CSV buffer does not contain data.".to_string(),
        ));
    };
    let raw_headers_rec = first_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    let raw_headers: Vec<String> = raw_headers_rec.iter().map(|h| h.to_string()).collect();
    let clean_headers: Vec<String> = raw_headers.iter().map(|h| format_header_name(h)).collect();

    let mut rows: Vec<Row> = Vec::new();
    for rec_res in records {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        let mut row = Row::with_capacity(clean_headers.len());
        for (i, clean_h) in clean_headers.iter().enumerate() {
            let val = rec.get(i).unwrap_or("").to_string();
            row.insert(clean_h.clone(), val);
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(SmartVaError::ExecutionError(
            "Input CSV buffer does not contain data.".to_string(),
        ));
    }
    run_smartva_in_memory(&clean_headers, &rows, options)
}

/// Read an input CSV file and score it 100% in memory without writing any output CSVs or charts.
pub fn run_smartva_csv_in_memory(
    input_csv: &Path,
    options: &SmartVaRunOptions,
) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
    if !input_csv.exists() {
        return Err(SmartVaError::IoError(format!(
            "Input CSV file does not exist: {}",
            input_csv.display()
        )));
    }
    let csv_bytes = fs::read(input_csv).map_err(|e| {
        SmartVaError::IoError(format!("Failed to read {}: {}", input_csv.display(), e))
    })?;
    run_smartva_csv_bytes_in_memory(&csv_bytes, options)
}
