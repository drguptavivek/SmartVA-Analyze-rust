//! Pure-Rust RFC-8259 JSON parser and serializer for SmartVA JSON payloads.
//!
//! Enables `smartva-rust` (`SmartVaEngine`, `smartva_score_json` FFI, and `smartva-rs --json` /
//! `--stream-json`) to accept single-form or multi-form Verbal Autopsy JSON payloads and emit
//! structured JSON predictions without external dependencies.

use std::collections::HashMap;

use crate::{SmartVaCliDirectoryResult, SmartVaError, SmartVaPrediction, SmartVaRunOptions};

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            JsonValue::Object(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Convert a scalar JSON cell value into the string representation expected by SmartVA's
    /// pipeline (`WHOPrep` / `CommonPrep`).
    pub fn to_cell_string(&self) -> String {
        match self {
            JsonValue::Null => String::new(),
            JsonValue::Bool(b) => {
                if *b {
                    "1".to_string()
                } else {
                    "0".to_string()
                }
            }
            JsonValue::Number(n) => {
                if n.eq_ignore_ascii_case("nan") {
                    String::new()
                } else {
                    n.clone()
                }
            }
            JsonValue::Str(s) => {
                let clean: String = s.chars().filter(|&c| c != '\0').collect();
                if clean.trim().eq_ignore_ascii_case("nan") {
                    String::new()
                } else {
                    clean
                }
            }
            JsonValue::Array(items) => items
                .iter()
                .map(|v| v.to_cell_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
            JsonValue::Object(_) => String::new(),
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            JsonValue::Number(n) => match n.trim() {
                "1" => Some(true),
                "0" => Some(false),
                _ => None,
            },
            JsonValue::Str(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "t" | "yes" | "y" | "1" => Some(true),
                "false" | "f" | "no" | "n" | "0" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn parse_value(&mut self) -> Result<JsonValue, SmartVaError> {
        let Some(b) = self.peek() else {
            return Err(SmartVaError::ExecutionError(
                "Unexpected end of JSON input".to_string(),
            ));
        };
        match b {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(JsonValue::Str),
            b't' | b'f' => self.parse_bool(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(SmartVaError::ExecutionError(format!(
                "Unexpected JSON character '{}' at byte {}",
                b as char, self.pos
            ))),
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, SmartVaError> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(JsonValue::Null)
        } else {
            Err(SmartVaError::ExecutionError(format!(
                "Invalid JSON literal at byte {}",
                self.pos
            )))
        }
    }

    fn parse_bool(&mut self) -> Result<JsonValue, SmartVaError> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(JsonValue::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(JsonValue::Bool(false))
        } else {
            Err(SmartVaError::ExecutionError(format!(
                "Invalid JSON boolean at byte {}",
                self.pos
            )))
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, SmartVaError> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|e| SmartVaError::ExecutionError(e.to_string()))?;
        if s.is_empty() || s == "-" {
            return Err(SmartVaError::ExecutionError(format!(
                "Invalid JSON number at byte {}",
                start
            )));
        }
        Ok(JsonValue::Number(s.to_string()))
    }

    fn parse_string(&mut self) -> Result<String, SmartVaError> {
        self.skip_ws();
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err(SmartVaError::ExecutionError(format!(
                "Expected '\"' at byte {}",
                self.pos
            )));
        }
        self.pos += 1;
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == b'"' {
                return Ok(out);
            } else if b == b'\\' {
                if self.pos >= self.bytes.len() {
                    return Err(SmartVaError::ExecutionError(
                        "Unterminated JSON escape".to_string(),
                    ));
                }
                let esc = self.bytes[self.pos];
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let code = self.parse_hex4()?;
                        if (0xD800..=0xDBFF).contains(&code) {
                            if self.bytes.get(self.pos..self.pos + 2) == Some(b"\\u") {
                                self.pos += 2;
                                let low = self.parse_hex4()?;
                                let combined = 0x10000
                                    + (((code as u32 - 0xD800) << 10) | (low as u32 - 0xDC00));
                                if let Some(ch) = char::from_u32(combined) {
                                    out.push(ch);
                                }
                            }
                        } else if let Some(ch) = char::from_u32(code as u32) {
                            out.push(ch);
                        }
                    }
                    other => out.push(other as char),
                }
            } else if b < 0x80 {
                out.push(b as char);
            } else {
                // Decode UTF-8 multi-byte sequence
                let start = self.pos - 1;
                let seq_len = if b & 0b1110_0000 == 0b1100_0000 {
                    2
                } else if b & 0b1111_0000 == 0b1110_0000 {
                    3
                } else {
                    4
                };
                let end = (start + seq_len).min(self.bytes.len());
                self.pos = end;
                let s = std::str::from_utf8(&self.bytes[start..end])
                    .map_err(|e| SmartVaError::ExecutionError(e.to_string()))?;
                out.push_str(s);
            }
        }
        Err(SmartVaError::ExecutionError(
            "Unterminated JSON string".to_string(),
        ))
    }

    fn parse_hex4(&mut self) -> Result<u16, SmartVaError> {
        if self.pos + 4 > self.bytes.len() {
            return Err(SmartVaError::ExecutionError(
                "Incomplete \\uXXXX escape in JSON".to_string(),
            ));
        }
        let hex_str = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|e| SmartVaError::ExecutionError(e.to_string()))?;
        self.pos += 4;
        u16::from_str_radix(hex_str, 16)
            .map_err(|e| SmartVaError::ExecutionError(format!("Invalid \\u escape: {}", e)))
    }

    fn parse_array(&mut self) -> Result<JsonValue, SmartVaError> {
        self.skip_ws();
        self.pos += 1; // skip '['
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    return Err(SmartVaError::ExecutionError(format!(
                        "Expected ',' or ']' in JSON array at byte {}",
                        self.pos
                    )))
                }
            }
        }
        Ok(JsonValue::Array(items))
    }

    fn parse_object(&mut self) -> Result<JsonValue, SmartVaError> {
        self.skip_ws();
        self.pos += 1; // skip '{'
        let mut entries = Vec::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(entries));
        }
        loop {
            let key = self.parse_string()?;
            if self.peek() != Some(b':') {
                return Err(SmartVaError::ExecutionError(format!(
                    "Expected ':' after key '{}' in JSON object at byte {}",
                    key, self.pos
                )));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            entries.push((key, val));
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => {
                    return Err(SmartVaError::ExecutionError(format!(
                        "Expected ',' or '}}' in JSON object at byte {}",
                        self.pos
                    )))
                }
            }
        }
        Ok(JsonValue::Object(entries))
    }
}

pub fn parse_json(input: &str) -> Result<JsonValue, SmartVaError> {
    let trimmed = input.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Err(SmartVaError::ExecutionError(
            "Empty JSON payload".to_string(),
        ));
    }
    let mut parser = JsonParser::new(trimmed);
    let val = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos < parser.bytes.len() {
        return Err(SmartVaError::ExecutionError(format!(
            "Trailing characters after JSON value at byte {}",
            parser.pos
        )));
    }
    Ok(val)
}

fn apply_json_options(obj: &JsonValue, opts: &mut SmartVaRunOptions) {
    if let Some(c) = obj.get("country").and_then(|v| v.as_str()) {
        opts.country = c.to_string();
    }
    if let Some(b) = obj.get("hiv").and_then(|v| v.as_bool()) {
        opts.hiv = b;
    }
    if let Some(b) = obj.get("malaria").and_then(|v| v.as_bool()) {
        opts.malaria = b;
    }
    if let Some(b) = obj.get("hce").and_then(|v| v.as_bool()) {
        opts.hce = b;
    }
    if let Some(b) = obj
        .get("freetext")
        .or_else(|| obj.get("free_text"))
        .and_then(|v| v.as_bool())
    {
        opts.freetext = b;
    }
    if let Some(b) = obj.get("extended").and_then(|v| v.as_bool()) {
        opts.extended = b;
    }
}

/// Extract headers, rows, and optional overridden run options from a JSON payload.
///
/// Supported JSON payload shapes:
/// 1. Single VA form object: `{"sid": "VA-001", "Id10013": "yes", "ageInYears": 65, ...}`
/// 2. Array of VA form objects: `[{"sid": "VA-001", ...}, {"sid": "VA-002", ...}]`
/// 3. Envelope object with options + forms:
///    `{"country": "IND", "hiv": true, "malaria": true, "forms": [{"sid": ...}]}`
///    (accepts `"forms"`, `"rows"`, `"records"`, `"data"`, `"form"`, or `"payload"`).
pub fn parse_va_json_payload(
    json_str: &str,
    default_options: &SmartVaRunOptions,
) -> Result<(Vec<String>, Vec<HashMap<String, String>>, SmartVaRunOptions), SmartVaError> {
    let root = parse_json(json_str)?;
    let mut opts = default_options.clone();
    opts.figures = false;

    let form_values: Vec<&JsonValue> = match &root {
        JsonValue::Array(arr) => arr.iter().collect(),
        JsonValue::Object(_) => {
            apply_json_options(&root, &mut opts);
            if let Some(nested_opts) = root.get("options") {
                apply_json_options(nested_opts, &mut opts);
            }
            let container = root
                .get("forms")
                .or_else(|| root.get("rows"))
                .or_else(|| root.get("records"))
                .or_else(|| root.get("data"))
                .or_else(|| root.get("form"))
                .or_else(|| root.get("payload"));
            match container {
                Some(JsonValue::Array(arr)) => arr.iter().collect(),
                Some(obj @ JsonValue::Object(_)) => vec![obj],
                _ => vec![&root],
            }
        }
        _ => {
            return Err(SmartVaError::ExecutionError(
                "JSON payload must be a JSON object or an array of JSON objects".to_string(),
            ))
        }
    };

    if form_values.is_empty() {
        return Err(SmartVaError::ExecutionError(
            "JSON payload contains zero VA forms".to_string(),
        ));
    }

    let mut headers: Vec<String> = Vec::new();
    let mut header_seen = std::collections::HashSet::new();
    let mut rows: Vec<HashMap<String, String>> = Vec::with_capacity(form_values.len());

    for (idx, fv) in form_values.iter().enumerate() {
        let Some(entries) = fv.as_object() else {
            return Err(SmartVaError::ExecutionError(format!(
                "Element {} in JSON payload is not a JSON object",
                idx
            )));
        };
        let mut row = HashMap::with_capacity(entries.len() + 1);
        for (k, v) in entries {
            if matches!(v, JsonValue::Object(_)) {
                continue;
            }
            if header_seen.insert(k.clone()) {
                headers.push(k.clone());
            }
            row.insert(k.clone(), v.to_cell_string());
        }
        if !row.contains_key("sid") {
            if header_seen.insert("sid".to_string()) {
                headers.push("sid".to_string());
            }
            row.insert("sid".to_string(), format!("json-row-{}", idx + 1));
        }
        rows.push(row);
    }

    Ok((headers, rows, opts))
}

/// Convert an input JSON payload (single VA object, array of VA objects, or envelope with options)
/// into an in-memory CSV byte buffer (`Vec<u8>`) along with the resolved [`SmartVaRunOptions`].
pub fn json_payload_to_csv_bytes(
    json_str: &str,
    default_options: &SmartVaRunOptions,
) -> Result<(Vec<u8>, SmartVaRunOptions), SmartVaError> {
    let (headers, rows, opts) = parse_va_json_payload(json_str, default_options)?;
    let mut wtr = csv::WriterBuilder::new().from_writer(Vec::with_capacity(
        headers.len() * 16 + rows.len() * headers.len() * 8,
    ));
    wtr.write_record(&headers)
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    let mut record = Vec::with_capacity(headers.len());
    for row in &rows {
        record.clear();
        for h in &headers {
            record.push(row.get(h).map(|s| s.as_str()).unwrap_or(""));
        }
        wtr.write_record(&record)
            .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    }
    let csv_bytes = wtr
        .into_inner()
        .map_err(|e| SmartVaError::IoError(e.to_string()))?;
    Ok((csv_bytes, opts))
}

fn push_escaped_json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_str_field(out: &mut String, key: &str, val: &str, trailing_comma: bool) {
    push_escaped_json_str(out, key);
    out.push(':');
    push_escaped_json_str(out, val);
    if trailing_comma {
        out.push(',');
    }
}

fn push_opt_str_field(out: &mut String, key: &str, val: &Option<String>, trailing_comma: bool) {
    push_escaped_json_str(out, key);
    out.push(':');
    match val {
        Some(s) => push_escaped_json_str(out, s),
        None => out.push_str("null"),
    }
    if trailing_comma {
        out.push(',');
    }
}

pub fn serialize_prediction_json(out: &mut String, p: &SmartVaPrediction) {
    out.push('{');
    push_str_field(out, "sid", &p.sid, true);
    push_opt_str_field(out, "module", &p.module, true);
    push_str_field(out, "national_id", &p.national_id, true);
    push_str_field(out, "name", &p.name, true);
    push_str_field(out, "name2", &p.name2, true);
    push_str_field(out, "surname", &p.surname, true);
    push_str_field(out, "surname2", &p.surname2, true);
    push_str_field(out, "geography1", &p.geography1, true);
    push_str_field(out, "geography2", &p.geography2, true);
    push_str_field(out, "geography3", &p.geography3, true);
    push_str_field(out, "geography4", &p.geography4, true);
    push_str_field(out, "geography5", &p.geography5, true);
    push_str_field(out, "cause34", &p.cause34, true);
    push_str_field(out, "cause list #", &p.cause_list_number, true);
    push_str_field(out, "icd10", &p.icd10, true);

    push_escaped_json_str(out, "age");
    out.push(':');
    match p.age {
        Some(a) if a.is_finite() => out.push_str(&format!("{}", a)),
        _ => out.push_str("null"),
    }
    out.push(',');

    push_escaped_json_str(out, "sex");
    out.push(':');
    match p.sex {
        Some(s) => out.push_str(&s.to_string()),
        None => out.push_str("null"),
    }
    out.push(',');

    push_str_field(out, "birth_date", &p.birth_date, true);
    push_str_field(out, "death_date", &p.death_date, true);
    push_str_field(out, "interview_date", &p.interview_date, true);
    push_opt_str_field(out, "cause1", &p.cause1, true);
    push_opt_str_field(out, "likelihood1", &p.likelihood1, true);
    push_opt_str_field(out, "key_symptom1", &p.key_symptom1, true);
    push_opt_str_field(out, "cause2", &p.cause2, true);
    push_opt_str_field(out, "likelihood2", &p.likelihood2, true);
    push_opt_str_field(out, "key_symptom2", &p.key_symptom2, true);
    push_opt_str_field(out, "cause3", &p.cause3, true);
    push_opt_str_field(out, "likelihood3", &p.likelihood3, true);
    push_opt_str_field(out, "key_symptom3", &p.key_symptom3, true);
    push_opt_str_field(out, "all_symptoms", &p.all_symptoms, false);
    out.push('}');
}

/// Convert an in-memory CSV table (`rows[0]` = header row, `rows[1..]` = data rows) directly
/// into a JSON array of row objects (`[{"col1":"val1","col2":"val2"}, ...]`).
pub fn serialize_csv_table_as_json_objects(out: &mut String, rows: &[Vec<String>]) {
    out.push('[');
    if rows.len() >= 2 {
        let headers = &rows[0];
        for (row_idx, row) in rows[1..].iter().enumerate() {
            if row_idx > 0 {
                out.push(',');
            }
            out.push('{');
            for (col_idx, h) in headers.iter().enumerate() {
                if col_idx > 0 {
                    out.push(',');
                }
                let val = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                push_escaped_json_str(out, h);
                out.push(':');
                push_escaped_json_str(out, val);
            }
            out.push('}');
        }
    }
    out.push(']');
}

/// Returns `true` if `table_name` is an individual/per-record CSV output (included in both
/// standard and extended JSON/Protobuf responses) as opposed to a cohort/CSMF table (included
/// only when `extended == true`).
pub fn is_individual_csv_table(table_name: &str) -> bool {
    table_name == "individual-cause-of-death.csv"
        || table_name.ends_with("-predictions.csv")
        || table_name.ends_with("-likelihoods.csv")
        || table_name.ends_with("-tariff-scores.csv")
        || table_name.ends_with("-tariff-ranks.csv")
        || table_name.ends_with("-external-ranks.csv")
}

fn push_csmf_map(
    out: &mut String,
    key: &str,
    map: &std::collections::BTreeMap<String, f64>,
    trailing_comma: bool,
) {
    push_escaped_json_str(out, key);
    out.push_str(":{");
    for (idx, (k, v)) in map.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        push_escaped_json_str(out, k);
        out.push(':');
        out.push_str(&format!("{}", v));
    }
    out.push('}');
    if trailing_comma {
        out.push(',');
    }
}

/// Serialize a [`SmartVaCliDirectoryResult`] into a compact JSON string by converting SmartVA's
/// in-memory CSV outputs directly to JSON.
///
/// - When `extended == false` (default / standard level, ideal for single-record `N = 1` or
///   per-record integrations): includes `total_vas`, `predictions`, `csv_outputs` for the
///   individual-level CSV tables (`individual-cause-of-death.csv`, `{module}-predictions.csv`,
///   `{module}-likelihoods.csv`, `{module}-tariff-scores.csv`, `{module}-tariff-ranks.csv`,
///   `{module}-external-ranks.csv`), and `warnings`. Does **not** include population CSMFs.
/// - When `extended == true` (cohort / population level): includes all standard fields plus
///   population CSMF maps (`csmf_all`, `csmf_adult`, `csmf_child`, `csmf_neonate`), all cohort
///   CSV tables in `csv_outputs` (`csmf.csv`, `{module}-csmf.csv`, `gbd-level1-csmf.csv`,
///   `causes-of-death.csv`, `VA-data-age-groupings.csv`, `{module}-endorsement-rates.csv`), and
///   `report` (`4-monitoring-and-quality/report.txt`).
pub fn serialize_result_json(res: &SmartVaCliDirectoryResult, extended: bool) -> String {
    let mut out = String::with_capacity(512 + res.predictions.len() * 512);
    out.push('{');
    push_escaped_json_str(&mut out, "total_vas");
    out.push(':');
    out.push_str(&res.total_vas.to_string());
    out.push(',');

    push_escaped_json_str(&mut out, "predictions");
    out.push_str(":[");
    for (idx, p) in res.predictions.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        serialize_prediction_json(&mut out, p);
    }
    out.push_str("],");

    push_escaped_json_str(&mut out, "csv_outputs");
    out.push_str(":{");
    let mut first_table = true;
    for (table_name, rows) in &res.csv_tables {
        if !extended && !is_individual_csv_table(table_name) {
            continue;
        }
        if !first_table {
            out.push(',');
        }
        first_table = false;
        push_escaped_json_str(&mut out, table_name);
        out.push(':');
        serialize_csv_table_as_json_objects(&mut out, rows);
    }
    out.push_str("},");

    push_escaped_json_str(&mut out, "warnings");
    out.push_str(":[");
    for (idx, w) in res.warnings.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        push_escaped_json_str(&mut out, w);
    }
    out.push(']');

    if extended {
        out.push(',');
        push_csmf_map(&mut out, "csmf_all", &res.csmf_all, true);
        push_csmf_map(&mut out, "csmf_adult", &res.csmf_adult, true);
        push_csmf_map(&mut out, "csmf_child", &res.csmf_child, true);
        push_csmf_map(&mut out, "csmf_neonate", &res.csmf_neonate, true);
        push_str_field(&mut out, "report", &res.report_text, false);
    }

    out.push('}');
    out
}
