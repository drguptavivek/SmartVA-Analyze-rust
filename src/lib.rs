//! 100% Pure Native Rust SmartVA-Analyze Engine (`smartva-rust` / `smartva-rs`).
//!
//! - **Zero runtime Python dependency** (`pyo3` and Python interpreter completely removed).
//! - **Zero JSON data conversion**: Static SmartVA domain modules (`smartva/data/*.py`) live
//!   1-for-1 as native Rust modules in [`data`] (`rust/src/data/*.rs`), and all SmartVA
//!   reference CSVs (`tariffs-{adult,child,neonate}.csv`, `validated-{adult,child,neonate}.csv`,
//!   `{adult,child,neonate}_undetermined_weights.csv`) and user input/output CSVs are read
//!   and written directly in Rust via the [`csv`] crate.
//! - Generates all `.xlsx` workbooks (`4-monitoring-and-quality/*-likelihoods.xlsx`) natively
//!   in Rust via [`rust_xlsxwriter`] ([`xlsx`]).
//! - Generates all `.png` figures (`3-graphs-and-tables/*.png`) natively in Rust via
//!   [`plotters`] ([`charts`]).
#![allow(clippy::type_complexity, clippy::needless_range_loop)]

pub mod charts;
pub mod data;
pub mod engine;
pub mod json;
pub mod ldap;
pub mod phmrc_source_prep;
pub mod protobuf;
pub mod stemmer;
pub mod xlsx;

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char, c_int};
use std::fmt;
use std::path::Path;
use std::ptr;

#[derive(Debug, Clone)]
pub enum SmartVaError {
    IoError(String),
    CsvError(String),
    ExecutionError(String),
}

impl fmt::Display for SmartVaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SmartVaError::IoError(msg) => write!(f, "IO error: {}", msg),
            SmartVaError::CsvError(msg) => write!(f, "CSV error: {}", msg),
            SmartVaError::ExecutionError(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for SmartVaError {}

#[derive(Debug, Clone)]
pub struct SmartVaRunOptions {
    pub country: String,
    pub hiv: bool,
    pub malaria: bool,
    pub hce: bool,
    pub freetext: bool,
    /// Generate `.png` charts and graphs (`--charts` / `--figures`).
    pub figures: bool,
    /// Generate the 4-folder CSV and `.xlsx` output directory (`--csv`).
    pub write_csv: bool,
    /// Include extended cohort-level outputs (CSMF tables, GBD Level 1 CSMF, age groupings, etc.)
    /// in JSON / Protobuf responses (`--extended`).
    pub extended: bool,
    pub language: String,
    pub legacy_format: bool,
}

impl Default for SmartVaRunOptions {
    fn default() -> Self {
        Self {
            country: "IND".to_string(),
            hiv: true,
            malaria: true,
            hce: true,
            freetext: true,
            figures: true,
            write_csv: true,
            extended: false,
            language: "english".to_string(),
            legacy_format: false,
        }
    }
}

/// Complete individual Verbal Autopsy prediction combining SmartVA's standard per-record CSV
/// outputs: `1-individual-cause-of-death/individual-cause-of-death.csv` and
/// `4-monitoring-and-quality/intermediate-files/{module}-likelihoods.csv`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SmartVaPrediction {
    pub sid: String,
    pub module: Option<String>,
    pub national_id: String,
    pub name: String,
    pub name2: String,
    pub surname: String,
    pub surname2: String,
    pub geography1: String,
    pub geography2: String,
    pub geography3: String,
    pub geography4: String,
    pub geography5: String,
    pub cause34: String,
    pub cause_list_number: String,
    pub icd10: String,
    pub age: Option<f64>,
    pub sex: Option<i32>,
    pub birth_date: String,
    pub death_date: String,
    pub interview_date: String,
    pub cause1: Option<String>,
    pub likelihood1: Option<String>,
    pub key_symptom1: Option<String>,
    pub cause2: Option<String>,
    pub likelihood2: Option<String>,
    pub key_symptom2: Option<String>,
    pub cause3: Option<String>,
    pub likelihood3: Option<String>,
    pub key_symptom3: Option<String>,
    pub all_symptoms: Option<String>,
}

/// Row from `2-csmf/csmf.csv` or `2-csmf/{adult,child,neonate}-csmf.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartVaCsmfRow {
    pub cause34: String,
    pub cause_list_number: Option<String>,
    pub icd10: String,
    pub all: f64,
    pub male: f64,
    pub female: f64,
}

/// Row from `2-csmf/gbd-level1-csmf.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartVaGbdCsmfRow {
    pub cause: String,
    pub cause_name: String,
    pub csmf: f64,
}

/// Row from `3-graphs-and-tables/causes-of-death.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartVaCausesOfDeathRow {
    pub module: String,
    pub cause34: String,
    pub age: String,
    pub sex: String,
    pub counts: usize,
}

/// Row from `4-monitoring-and-quality/VA-data-age-groupings.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartVaAgeGroupingRow {
    pub cause34: String,
    pub icd10: String,
    pub age_group: String,
    pub count: usize,
    pub male_count: usize,
    pub female_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SmartVaCliDirectoryResult {
    pub total_vas: usize,
    pub predictions: Vec<SmartVaPrediction>,
    pub csmf_all: BTreeMap<String, f64>,
    pub csmf_adult: BTreeMap<String, f64>,
    pub csmf_child: BTreeMap<String, f64>,
    pub csmf_neonate: BTreeMap<String, f64>,
    pub csmf_rows: Vec<SmartVaCsmfRow>,
    pub csmf_adult_rows: Vec<SmartVaCsmfRow>,
    pub csmf_child_rows: Vec<SmartVaCsmfRow>,
    pub csmf_neonate_rows: Vec<SmartVaCsmfRow>,
    pub gbd_level1_csmf_rows: Vec<SmartVaGbdCsmfRow>,
    pub causes_of_death_rows: Vec<SmartVaCausesOfDeathRow>,
    pub age_groupings_rows: Vec<SmartVaAgeGroupingRow>,
    /// In-memory CSV tables (`relative_path -> rows` where `rows[0]` is the header row
    /// and `rows[1..]` are the data rows).
    pub csv_tables: BTreeMap<String, Vec<Vec<String>>>,
    pub warnings: Vec<String>,
    pub report_text: String,
    pub xlsx_files_written: usize,
    pub png_charts_written: usize,
}

/// Pure-Rust SmartVA Engine with in-process cached Tariff 2.0 validation matrices.
#[derive(Debug, Clone, Default)]
pub struct SmartVaEngine;

impl SmartVaEngine {
    /// Create a new pure-Rust `SmartVaEngine` instance.
    pub fn new() -> Self {
        Self
    }

    /// Pre-warm the Adult, Child, and Neonate `TariffPrep` validation models in memory.
    pub fn warm_cache(&self) -> Result<(), SmartVaError> {
        engine::warm_all_models()
    }

    /// Return all 188 valid SmartVA country strings (`"Afghanistan (AFG)"`, etc.).
    pub fn list_countries(&self) -> Vec<&'static str> {
        data::countries::COUNTRIES.to_vec()
    }

    /// Execute the SmartVA-Analyze pipeline on `input_csv`.
    /// When `options.write_csv` is `true`, writes the 4 output folders into `output_dir`
    /// (and `.png` charts when `options.figures` is `true`).
    pub fn run_cli_directory(
        &self,
        input_csv: &Path,
        output_dir: &Path,
        options: &SmartVaRunOptions,
    ) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
        if !options.write_csv && !options.figures {
            engine::run_smartva_csv_in_memory(input_csv, options)
        } else {
            engine::run_smartva_pipeline_directory(input_csv, output_dir, options)
        }
    }

    /// Score an input CSV file 100% in memory (without writing CSVs or charts to disk)
    /// and return the structured [`SmartVaCliDirectoryResult`].
    pub fn score_csv(
        &self,
        input_csv: &Path,
        options: &SmartVaRunOptions,
    ) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
        engine::run_smartva_csv_in_memory(input_csv, options)
    }

    /// Score in-memory rows (`headers` and `rows` of string cells) 100% in memory.
    pub fn score_rows(
        &self,
        headers: &[String],
        rows: &[HashMap<String, String>],
        options: &SmartVaRunOptions,
    ) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
        engine::run_smartva_in_memory(headers, rows, options)
    }

    /// Score a single VA form (`HashMap<String, String>`) 100% in memory.
    pub fn score_row(
        &self,
        row: &HashMap<String, String>,
        options: &SmartVaRunOptions,
    ) -> Result<SmartVaPrediction, SmartVaError> {
        let mut headers: Vec<String> = row.keys().cloned().collect();
        headers.sort();
        if !row.contains_key("sid") {
            headers.insert(0, "sid".to_string());
        }
        let mut single_row = row.clone();
        single_row
            .entry("sid".to_string())
            .or_insert_with(|| "row-1".to_string());
        let res = engine::run_smartva_in_memory(&headers, &[single_row], options)?;
        res.predictions.into_iter().next().ok_or_else(|| {
            SmartVaError::ExecutionError(
                "VA record could not be scored (refused consent or invalid/missing age)."
                    .to_string(),
            )
        })
    }

    /// Convert an input JSON payload (single VA object, array of VA objects, or envelope with options)
    /// into an in-memory CSV byte buffer (`Vec<u8>`) along with the resolved [`SmartVaRunOptions`].
    pub fn json_to_csv(
        &self,
        json_payload: &str,
        default_options: &SmartVaRunOptions,
    ) -> Result<(Vec<u8>, SmartVaRunOptions), SmartVaError> {
        json::json_payload_to_csv_bytes(json_payload, default_options)
    }

    /// Score an in-memory CSV byte buffer (`&[u8]`) 100% in memory without writing any files to disk.
    pub fn score_csv_bytes(
        &self,
        csv_bytes: &[u8],
        options: &SmartVaRunOptions,
    ) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
        engine::run_smartva_csv_bytes_in_memory(csv_bytes, options)
    }

    /// Score a JSON payload in memory by converting the JSON input to an in-memory CSV buffer,
    /// running the SmartVA CSV pipeline in memory, and returning the structured
    /// [`SmartVaCliDirectoryResult`] along with the effective `extended` flag.
    pub fn score_json_result(
        &self,
        json_payload: &str,
        default_options: &SmartVaRunOptions,
    ) -> Result<(SmartVaCliDirectoryResult, bool), SmartVaError> {
        let (csv_bytes, opts) = json::json_payload_to_csv_bytes(json_payload, default_options)?;
        let res = engine::run_smartva_csv_bytes_in_memory(&csv_bytes, &opts)?;
        Ok((res, opts.extended))
    }

    /// Score a JSON payload (single VA object, array of VA objects, or envelope with options)
    /// and return the analysis result as a JSON string.
    ///
    /// - Standard level (`options.extended == false`): returns individual predictions,
    ///   likelihoods, key symptoms, and per-cause tariff scores/ranks/probabilities.
    /// - Extended level (`options.extended == true` or `"extended": true` in JSON): also
    ///   includes cohort CSMFs, GBD Level 1 CSMF, causes-of-death table, age groupings, and report.
    pub fn score_json(
        &self,
        json_payload: &str,
        default_options: &SmartVaRunOptions,
    ) -> Result<String, SmartVaError> {
        let (res, extended) = self.score_json_result(json_payload, default_options)?;
        Ok(json::serialize_result_json(&res, extended))
    }

    /// Score a JSON payload and return the analysis result encoded as Protocol Buffers (`proto3` wire format).
    pub fn score_json_to_protobuf(
        &self,
        json_payload: &str,
        default_options: &SmartVaRunOptions,
    ) -> Result<Vec<u8>, SmartVaError> {
        let (res, extended) = self.score_json_result(json_payload, default_options)?;
        Ok(protobuf::encode_result_protobuf(&res, extended))
    }

    /// Serialize a [`SmartVaCliDirectoryResult`] to JSON (`extended = false` for standard individual output,
    /// `extended = true` to include cohort CSMFs and summary tables).
    pub fn to_json(&self, result: &SmartVaCliDirectoryResult, extended: bool) -> String {
        json::serialize_result_json(result, extended)
    }

    /// Serialize a [`SmartVaCliDirectoryResult`] to Protocol Buffers (`proto3` binary wire format).
    pub fn to_protobuf(&self, result: &SmartVaCliDirectoryResult, extended: bool) -> Vec<u8> {
        protobuf::encode_result_protobuf(result, extended)
    }
}

// ============================================================================
// C-ABI FFI Exports for external callers (C, C++, Swift, Python ctypes, R, etc.)
// ============================================================================

/// Pre-warm the in-process Tariff 2.0 validation matrices via C-ABI.
/// Returns `0` on success, `-1` on error.
#[unsafe(no_mangle)]
pub extern "C" fn smartva_warm_cache() -> c_int {
    match engine::warm_all_models() {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Run the full SmartVA-Analyze pipeline on `input_csv_path` and write all output files
/// into `output_dir_path` via C-ABI.
///
/// Returns `0` on success, non-zero on error. If `error_out` is non-null, writes a newly
/// allocated error string on failure (free with [`smartva_free_string`]).
///
/// # Safety
/// `input_csv_path`, `output_dir_path`, and `country` must be valid null-terminated UTF-8 C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smartva_run_directory(
    input_csv_path: *const c_char,
    output_dir_path: *const c_char,
    country: *const c_char,
    hiv: c_int,
    malaria: c_int,
    hce: c_int,
    freetext: c_int,
    figures: c_int,
    error_out: *mut *mut c_char,
) -> c_int {
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
    let parse_str = |p: *const c_char| -> Result<String, String> {
        if p.is_null() {
            return Err("Null pointer argument".to_string());
        }
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| e.to_string())
    };
    let res = (|| -> Result<(), String> {
        let in_str = parse_str(input_csv_path)?;
        let out_str = parse_str(output_dir_path)?;
        let country_str = parse_str(country)?;
        let opts = SmartVaRunOptions {
            country: country_str,
            hiv: hiv != 0,
            malaria: malaria != 0,
            hce: hce != 0,
            freetext: freetext != 0,
            figures: figures != 0,
            write_csv: true,
            extended: true,
            language: "english".to_string(),
            legacy_format: false,
        };
        engine::run_smartva_pipeline_directory(Path::new(&in_str), Path::new(&out_str), &opts)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })();

    match res {
        Ok(()) => 0,
        Err(msg) => {
            if !error_out.is_null()
                && let Ok(cstr) = CString::new(msg)
            {
                unsafe { *error_out = cstr.into_raw() };
            }
            2
        }
    }
}

/// Score a JSON payload via C-ABI and return the analysis result as a newly allocated JSON C-string
/// in `result_json_out` (free with [`smartva_free_string`]).
///
/// - Set `extended = 0` for standard individual VA output (`1-individual-cause-of-death.csv` +
///   `{module}-likelihoods.csv`).
/// - Set `extended = 1` to also include cohort-level outputs (`2-csmf/*.csv`, `3-graphs-and-tables/causes-of-death.csv`,
///   `4-monitoring-and-quality/VA-data-age-groupings.csv`, `report.txt`).
///
/// # Safety
/// `json_payload` and `country` must be valid null-terminated UTF-8 C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smartva_score_json(
    json_payload: *const c_char,
    country: *const c_char,
    hiv: c_int,
    malaria: c_int,
    hce: c_int,
    freetext: c_int,
    extended: c_int,
    result_json_out: *mut *mut c_char,
    error_out: *mut *mut c_char,
) -> c_int {
    if !result_json_out.is_null() {
        unsafe { *result_json_out = ptr::null_mut() };
    }
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
    let parse_str = |p: *const c_char| -> Result<String, String> {
        if p.is_null() {
            return Err("Null pointer argument".to_string());
        }
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| e.to_string())
    };
    let res = (|| -> Result<String, String> {
        let payload_str = parse_str(json_payload)?;
        let country_str = parse_str(country)?;
        let opts = SmartVaRunOptions {
            country: country_str,
            hiv: hiv != 0,
            malaria: malaria != 0,
            hce: hce != 0,
            freetext: freetext != 0,
            figures: false,
            write_csv: false,
            extended: extended != 0,
            language: "english".to_string(),
            legacy_format: false,
        };
        SmartVaEngine::new()
            .score_json(&payload_str, &opts)
            .map_err(|e| e.to_string())
    })();

    match res {
        Ok(json_out) => {
            if !result_json_out.is_null()
                && let Ok(cstr) = CString::new(json_out)
            {
                unsafe { *result_json_out = cstr.into_raw() };
            }
            0
        }
        Err(msg) => {
            if !error_out.is_null()
                && let Ok(cstr) = CString::new(msg)
            {
                unsafe { *error_out = cstr.into_raw() };
            }
            2
        }
    }
}

/// Simplified single-return-pointer C-ABI export for higher-level FFI runtimes
/// (Swift, Flutter `dart:ffi`, Kotlin JNA, Node.js `koffi`, Bun `bun:ffi`, Deno `Deno.dlopen`, Python `ctypes`).
///
/// Returns a newly allocated UTF-8 JSON C-string containing the analysis response
/// (or `{"error":"..."}` on failure). Always free the returned pointer with [`smartva_free_string`].
///
/// # Safety
/// `json_payload` and `country` must be valid null-terminated UTF-8 C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smartva_score_json_simple(
    json_payload: *const c_char,
    country: *const c_char,
    hiv: c_int,
    malaria: c_int,
    hce: c_int,
    freetext: c_int,
    extended: c_int,
) -> *mut c_char {
    let mut out_ptr: *mut c_char = ptr::null_mut();
    let mut err_ptr: *mut c_char = ptr::null_mut();
    let rc = unsafe {
        smartva_score_json(
            json_payload,
            country,
            hiv,
            malaria,
            hce,
            freetext,
            extended,
            &mut out_ptr,
            &mut err_ptr,
        )
    };
    if rc == 0 && !out_ptr.is_null() {
        out_ptr
    } else {
        let err_msg = if !err_ptr.is_null() {
            let s = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
            unsafe { smartva_free_string(err_ptr) };
            s
        } else {
            "Unknown error in smartva_score_json_simple".to_string()
        };
        let escaped = err_msg.replace('\\', "\\\\").replace('"', "\\\"");
        CString::new(format!("{{\"error\":\"{escaped}\"}}"))
            .unwrap_or_default()
            .into_raw()
    }
}

/// Score a JSON payload via C-ABI and return the analysis result as a newly allocated Protobuf
/// (`proto3` binary wire format) buffer in `result_bytes_out` and `result_len_out`
/// (free with [`smartva_free_bytes`]).
///
/// # Safety
/// `json_payload` and `country` must be valid null-terminated UTF-8 C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smartva_score_protobuf(
    json_payload: *const c_char,
    country: *const c_char,
    hiv: c_int,
    malaria: c_int,
    hce: c_int,
    freetext: c_int,
    extended: c_int,
    result_bytes_out: *mut *mut u8,
    result_len_out: *mut usize,
    error_out: *mut *mut c_char,
) -> c_int {
    if !result_bytes_out.is_null() {
        unsafe { *result_bytes_out = ptr::null_mut() };
    }
    if !result_len_out.is_null() {
        unsafe { *result_len_out = 0 };
    }
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
    let parse_str = |p: *const c_char| -> Result<String, String> {
        if p.is_null() {
            return Err("Null pointer argument".to_string());
        }
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .map(|s| s.to_string())
            .map_err(|e| e.to_string())
    };
    let res = (|| -> Result<Vec<u8>, String> {
        let payload_str = parse_str(json_payload)?;
        let country_str = parse_str(country)?;
        let opts = SmartVaRunOptions {
            country: country_str,
            hiv: hiv != 0,
            malaria: malaria != 0,
            hce: hce != 0,
            freetext: freetext != 0,
            figures: false,
            write_csv: false,
            extended: extended != 0,
            language: "english".to_string(),
            legacy_format: false,
        };
        SmartVaEngine::new()
            .score_json_to_protobuf(&payload_str, &opts)
            .map_err(|e| e.to_string())
    })();

    match res {
        Ok(bytes) => {
            let boxed = bytes.into_boxed_slice();
            let len = boxed.len();
            let raw_ptr = Box::into_raw(boxed) as *mut u8;
            if !result_bytes_out.is_null() {
                unsafe { *result_bytes_out = raw_ptr };
            }
            if !result_len_out.is_null() {
                unsafe { *result_len_out = len };
            }
            0
        }
        Err(msg) => {
            if !error_out.is_null()
                && let Ok(cstr) = CString::new(msg)
            {
                unsafe { *error_out = cstr.into_raw() };
            }
            2
        }
    }
}

/// Free a C string allocated by [`smartva_run_directory`] or [`smartva_score_json`].
///
/// # Safety
/// `ptr` must be null or a pointer returned by this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smartva_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

/// Free a byte buffer allocated by [`smartva_score_protobuf`].
///
/// # Safety
/// `ptr` and `len` must match a buffer returned by [`smartva_score_protobuf`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn smartva_free_bytes(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        unsafe {
            let slice_ptr = ptr::slice_from_raw_parts_mut(ptr, len);
            let _ = Box::from_raw(slice_ptr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_rust_country_list_and_warm_cache() {
        let engine = SmartVaEngine::new();
        let countries = engine.list_countries();
        assert_eq!(countries.len(), 188);
        assert!(countries.iter().any(|c| c.contains("(IND)")));
        engine.warm_cache().expect("Failed to warm pure-Rust models");
    }

    #[test]
    fn test_json_and_protobuf_standard_vs_extended_levels() {
        let engine = SmartVaEngine::new();
        let payload = r#"{
            "sid": "VA-JSON-001",
            "Id10013": "yes",
            "Id10019": "male",
            "ageInYears": 62,
            "Id10004": "dry",
            "Id10147": "yes",
            "Id10148": 14,
            "Id10153": "yes",
            "Id10154": 5
        }"#;

        // 1. Standard level (extended = false): individual CSV outputs only, no CSMF
        let std_opts = SmartVaRunOptions {
            country: "IND".to_string(),
            extended: false,
            ..Default::default()
        };
        let std_json_str = engine
            .score_json(payload, &std_opts)
            .expect("Standard JSON scoring failed");
        let parsed_std = json::parse_json(&std_json_str).expect("Valid JSON");
        assert_eq!(
            parsed_std.get("total_vas").unwrap().to_cell_string(),
            "1"
        );
        let preds = parsed_std
            .get("predictions")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(preds.len(), 1);
        assert_eq!(preds[0].get("sid").unwrap().as_str(), Some("VA-JSON-001"));
        assert_eq!(preds[0].get("module").unwrap().as_str(), Some("adult"));
        assert!(preds[0].get("cause34").unwrap().as_str().is_some());
        assert!(preds[0].get("cause list #").unwrap().as_str().is_some());

        // Verify standard csv_outputs contains individual CSV tables and NO cohort CSMF tables
        let std_csv_outputs = parsed_std.get("csv_outputs").unwrap();
        assert!(
            std_csv_outputs
                .get("individual-cause-of-death.csv")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.len() == 1)
        );
        assert!(
            std_csv_outputs
                .get("adult-predictions.csv")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.len() == 1)
        );
        assert!(
            std_csv_outputs
                .get("adult-likelihoods.csv")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.len() == 1)
        );
        assert!(
            std_csv_outputs
                .get("adult-tariff-scores.csv")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.len() == 1)
        );
        assert!(
            std_csv_outputs
                .get("adult-tariff-ranks.csv")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.len() == 1)
        );
        assert!(
            std_csv_outputs
                .get("adult-external-ranks.csv")
                .and_then(|v| v.as_array())
                .is_some_and(|a| a.len() == 1)
        );
        assert!(std_csv_outputs.get("csmf.csv").is_none());
        assert!(std_csv_outputs.get("adult-csmf.csv").is_none());
        assert!(parsed_std.get("csmf_all").is_none());

        // 2. Extended level (extended = true): includes cohort CSMFs and all CSV tables
        let mut ext_opts = std_opts.clone();
        ext_opts.extended = true;
        let ext_json_str = engine
            .score_json(payload, &ext_opts)
            .expect("Extended JSON scoring failed");
        let parsed_ext = json::parse_json(&ext_json_str).expect("Valid Extended JSON");
        let ext_csv_outputs = parsed_ext.get("csv_outputs").unwrap();
        assert!(ext_csv_outputs.get("csmf.csv").is_some());
        assert!(ext_csv_outputs.get("adult-csmf.csv").is_some());
        assert!(ext_csv_outputs.get("gbd-level1-csmf.csv").is_some());
        assert!(ext_csv_outputs.get("causes-of-death.csv").is_some());
        assert!(ext_csv_outputs.get("VA-data-age-groupings.csv").is_some());
        assert!(ext_csv_outputs.get("adult-endorsement-rates.csv").is_some());
        assert!(parsed_ext.get("csmf_all").is_some());
        assert!(parsed_ext.get("report").is_some());

        // 3. Protobuf round-trip (standard & extended)
        let pb_std = engine
            .score_json_to_protobuf(payload, &std_opts)
            .expect("Standard Protobuf failed");
        let decoded_std =
            protobuf::decode_result_protobuf(&pb_std).expect("Decode standard Protobuf");
        assert_eq!(decoded_std.total_vas, 1);
        assert_eq!(decoded_std.predictions[0].sid, "VA-JSON-001");
        assert!(decoded_std
            .csv_tables
            .contains_key("individual-cause-of-death.csv"));
        assert!(decoded_std.csv_tables.contains_key("adult-likelihoods.csv"));
        assert!(!decoded_std.csv_tables.contains_key("csmf.csv"));
        assert!(decoded_std.csmf_all.is_empty());

        let pb_ext = engine
            .score_json_to_protobuf(payload, &ext_opts)
            .expect("Extended Protobuf failed");
        let decoded_ext =
            protobuf::decode_result_protobuf(&pb_ext).expect("Decode extended Protobuf");
        assert_eq!(decoded_ext.total_vas, 1);
        assert!(decoded_ext.csv_tables.contains_key("csmf.csv"));
        assert!(!decoded_ext.csmf_all.is_empty());
        assert!(!decoded_ext.report_text.is_empty());
    }

    #[test]
    fn test_c_abi_ffi_exports_and_simple_helper() {
        assert_eq!(smartva_warm_cache(), 0);

        let payload = CString::new(
            r#"{"sid":"VA-FFI-001","Id10013":"yes","Id10019":"female","ageInYears":34,"Id10147":"yes","Id10310":"yes"}"#,
        )
        .unwrap();
        let country = CString::new("IND").unwrap();

        // 1. Test smartva_score_json_simple
        let ptr = unsafe {
            smartva_score_json_simple(payload.as_ptr(), country.as_ptr(), 1, 1, 1, 1, 1)
        };
        assert!(!ptr.is_null());
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        unsafe { smartva_free_string(ptr) };
        let parsed = json::parse_json(&json_str).expect("Valid FFI JSON");
        assert_eq!(parsed.get("total_vas").unwrap().to_cell_string(), "1");
        assert!(parsed.get("csmf_all").is_some());

        // 2. Test smartva_score_protobuf
        let mut pb_ptr: *mut u8 = ptr::null_mut();
        let mut pb_len: usize = 0;
        let mut err_ptr: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            smartva_score_protobuf(
                payload.as_ptr(),
                country.as_ptr(),
                1,
                1,
                1,
                1,
                1,
                &mut pb_ptr,
                &mut pb_len,
                &mut err_ptr,
            )
        };
        assert_eq!(rc, 0);
        assert!(!pb_ptr.is_null());
        assert!(pb_len > 0);
        assert!(err_ptr.is_null());
        let slice = unsafe { std::slice::from_raw_parts(pb_ptr, pb_len) };
        let decoded = protobuf::decode_result_protobuf(slice).expect("Valid FFI Protobuf");
        assert_eq!(decoded.total_vas, 1);
        assert_eq!(decoded.predictions[0].sid, "VA-FFI-001");
        unsafe { smartva_free_bytes(pb_ptr, pb_len) };
    }
}
