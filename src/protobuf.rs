//! Pure-Rust Protocol Buffers v3 wire-format encoder and decoder for SmartVA results.
//!
//! Matches `proto/smartva.proto` (`package smartva; message SmartVaResult`) with zero external
//! dependencies or `protoc` build steps.

use std::collections::BTreeMap;

use crate::json::is_individual_csv_table;
use crate::{SmartVaCliDirectoryResult, SmartVaError, SmartVaPrediction};

const WIRE_VARINT: u32 = 0;
const WIRE_64BIT: u32 = 1;
const WIRE_LEN: u32 = 2;

fn write_varint(buf: &mut Vec<u8>, mut val: u64) {
    while val >= 0x80 {
        buf.push(((val as u8) & 0x7F) | 0x80);
        val >>= 7;
    }
    buf.push(val as u8);
}

fn write_tag(buf: &mut Vec<u8>, field_number: u32, wire_type: u32) {
    write_varint(buf, ((field_number << 3) | wire_type) as u64);
}

fn write_string_field(buf: &mut Vec<u8>, field_number: u32, s: &str) {
    if s.is_empty() {
        return;
    }
    write_tag(buf, field_number, WIRE_LEN);
    write_varint(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

fn write_always_string_field(buf: &mut Vec<u8>, field_number: u32, s: &str) {
    write_tag(buf, field_number, WIRE_LEN);
    write_varint(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

fn write_opt_string_field(buf: &mut Vec<u8>, field_number: u32, opt: &Option<String>) {
    if let Some(s) = opt {
        write_string_field(buf, field_number, s);
    }
}

fn write_double_field(buf: &mut Vec<u8>, field_number: u32, val: f64) {
    write_tag(buf, field_number, WIRE_64BIT);
    buf.extend_from_slice(&val.to_le_bytes());
}

fn write_int32_field(buf: &mut Vec<u8>, field_number: u32, val: i32) {
    write_tag(buf, field_number, WIRE_VARINT);
    write_varint(buf, (val as i64) as u64);
}

fn write_submessage(buf: &mut Vec<u8>, field_number: u32, sub: &[u8]) {
    write_tag(buf, field_number, WIRE_LEN);
    write_varint(buf, sub.len() as u64);
    buf.extend_from_slice(sub);
}

fn write_csmf_map(buf: &mut Vec<u8>, field_number: u32, map: &BTreeMap<String, f64>) {
    let mut entry = Vec::with_capacity(64);
    for (k, &v) in map {
        entry.clear();
        write_string_field(&mut entry, 1, k);
        write_double_field(&mut entry, 2, v);
        write_submessage(buf, field_number, &entry);
    }
}

fn encode_prediction(p: &SmartVaPrediction) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    write_string_field(&mut buf, 1, &p.sid);
    write_opt_string_field(&mut buf, 2, &p.module);
    write_string_field(&mut buf, 3, &p.national_id);
    write_string_field(&mut buf, 4, &p.name);
    write_string_field(&mut buf, 5, &p.name2);
    write_string_field(&mut buf, 6, &p.surname);
    write_string_field(&mut buf, 7, &p.surname2);
    write_string_field(&mut buf, 8, &p.geography1);
    write_string_field(&mut buf, 9, &p.geography2);
    write_string_field(&mut buf, 10, &p.geography3);
    write_string_field(&mut buf, 11, &p.geography4);
    write_string_field(&mut buf, 12, &p.geography5);
    write_string_field(&mut buf, 13, &p.cause34);
    write_string_field(&mut buf, 14, &p.cause_list_number);
    write_string_field(&mut buf, 15, &p.icd10);
    if let Some(age) = p.age {
        if age.is_finite() {
            write_double_field(&mut buf, 16, age);
        }
    }
    if let Some(sex) = p.sex {
        write_int32_field(&mut buf, 17, sex);
    }
    write_string_field(&mut buf, 18, &p.birth_date);
    write_string_field(&mut buf, 19, &p.death_date);
    write_string_field(&mut buf, 20, &p.interview_date);
    write_opt_string_field(&mut buf, 21, &p.cause1);
    write_opt_string_field(&mut buf, 22, &p.likelihood1);
    write_opt_string_field(&mut buf, 23, &p.key_symptom1);
    write_opt_string_field(&mut buf, 24, &p.cause2);
    write_opt_string_field(&mut buf, 25, &p.likelihood2);
    write_opt_string_field(&mut buf, 26, &p.key_symptom2);
    write_opt_string_field(&mut buf, 27, &p.cause3);
    write_opt_string_field(&mut buf, 28, &p.likelihood3);
    write_opt_string_field(&mut buf, 29, &p.key_symptom3);
    write_opt_string_field(&mut buf, 30, &p.all_symptoms);
    buf
}

fn encode_csv_table(name: &str, rows: &[Vec<String>]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    write_string_field(&mut buf, 1, name);
    if let Some(headers) = rows.first() {
        for h in headers {
            write_always_string_field(&mut buf, 2, h);
        }
        let mut row_buf = Vec::with_capacity(128);
        for r in &rows[1..] {
            row_buf.clear();
            for cell in r {
                write_always_string_field(&mut row_buf, 1, cell);
            }
            write_submessage(&mut buf, 3, &row_buf);
        }
    }
    buf
}

/// Encode a [`SmartVaCliDirectoryResult`] into Protocol Buffers v3 binary wire format
/// (`smartva.SmartVaResult` in `proto/smartva.proto`).
pub fn encode_result_protobuf(res: &SmartVaCliDirectoryResult, extended: bool) -> Vec<u8> {
    let mut buf = Vec::with_capacity(512 + res.predictions.len() * 256);
    if res.total_vas > 0 {
        write_tag(&mut buf, 1, WIRE_VARINT);
        write_varint(&mut buf, res.total_vas as u64);
    }
    for p in &res.predictions {
        let sub = encode_prediction(p);
        write_submessage(&mut buf, 2, &sub);
    }
    for (name, rows) in &res.csv_tables {
        if !extended && !is_individual_csv_table(name) {
            continue;
        }
        let sub = encode_csv_table(name, rows);
        write_submessage(&mut buf, 3, &sub);
    }
    for w in &res.warnings {
        write_string_field(&mut buf, 4, w);
    }
    if extended {
        write_csmf_map(&mut buf, 5, &res.csmf_all);
        write_csmf_map(&mut buf, 6, &res.csmf_adult);
        write_csmf_map(&mut buf, 7, &res.csmf_child);
        write_csmf_map(&mut buf, 8, &res.csmf_neonate);
        write_string_field(&mut buf, 9, &res.report_text);
    }
    buf
}

struct ProtoReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ProtoReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn read_varint(&mut self) -> Result<u64, SmartVaError> {
        let mut result = 0u64;
        let mut shift = 0u32;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            result |= ((b & 0x7F) as u64) << shift;
            if (b & 0x80) == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err(SmartVaError::ExecutionError(
                    "Protobuf varint overflow".to_string(),
                ));
            }
        }
        Err(SmartVaError::ExecutionError(
            "Unexpected EOF while reading Protobuf varint".to_string(),
        ))
    }

    fn read_tag(&mut self) -> Result<(u32, u32), SmartVaError> {
        let v = self.read_varint()?;
        Ok(((v >> 3) as u32, (v & 0x07) as u32))
    }

    fn read_bytes(&mut self) -> Result<&'a [u8], SmartVaError> {
        let len = self.read_varint()? as usize;
        if self.pos + len > self.bytes.len() {
            return Err(SmartVaError::ExecutionError(
                "Unexpected EOF while reading Protobuf length-delimited bytes".to_string(),
            ));
        }
        let slice = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn read_string(&mut self) -> Result<String, SmartVaError> {
        let slice = self.read_bytes()?;
        std::str::from_utf8(slice)
            .map(|s| s.to_string())
            .map_err(|e| SmartVaError::ExecutionError(format!("Invalid UTF-8 in Protobuf: {}", e)))
    }

    fn read_double(&mut self) -> Result<f64, SmartVaError> {
        if self.pos + 8 > self.bytes.len() {
            return Err(SmartVaError::ExecutionError(
                "Unexpected EOF while reading Protobuf double".to_string(),
            ));
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(&self.bytes[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(f64::from_le_bytes(arr))
    }

    fn skip_field(&mut self, wire_type: u32) -> Result<(), SmartVaError> {
        match wire_type {
            WIRE_VARINT => {
                let _ = self.read_varint()?;
                Ok(())
            }
            WIRE_64BIT => {
                if self.pos + 8 > self.bytes.len() {
                    return Err(SmartVaError::ExecutionError(
                        "Unexpected EOF while skipping 64-bit Protobuf field".to_string(),
                    ));
                }
                self.pos += 8;
                Ok(())
            }
            WIRE_LEN => {
                let _ = self.read_bytes()?;
                Ok(())
            }
            5 => {
                if self.pos + 4 > self.bytes.len() {
                    return Err(SmartVaError::ExecutionError(
                        "Unexpected EOF while skipping 32-bit Protobuf field".to_string(),
                    ));
                }
                self.pos += 4;
                Ok(())
            }
            other => Err(SmartVaError::ExecutionError(format!(
                "Unsupported Protobuf wire type {}",
                other
            ))),
        }
    }
}

fn decode_prediction(bytes: &[u8]) -> Result<SmartVaPrediction, SmartVaError> {
    let mut r = ProtoReader::new(bytes);
    let mut p = SmartVaPrediction {
        sid: String::new(),
        module: None,
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
        cause34: String::new(),
        cause_list_number: String::new(),
        icd10: String::new(),
        age: None,
        sex: None,
        birth_date: String::new(),
        death_date: String::new(),
        interview_date: String::new(),
        cause1: None,
        likelihood1: None,
        key_symptom1: None,
        cause2: None,
        likelihood2: None,
        key_symptom2: None,
        cause3: None,
        likelihood3: None,
        key_symptom3: None,
        all_symptoms: None,
    };
    while !r.is_eof() {
        let (field, wt) = r.read_tag()?;
        match (field, wt) {
            (1, WIRE_LEN) => p.sid = r.read_string()?,
            (2, WIRE_LEN) => p.module = Some(r.read_string()?),
            (3, WIRE_LEN) => p.national_id = r.read_string()?,
            (4, WIRE_LEN) => p.name = r.read_string()?,
            (5, WIRE_LEN) => p.name2 = r.read_string()?,
            (6, WIRE_LEN) => p.surname = r.read_string()?,
            (7, WIRE_LEN) => p.surname2 = r.read_string()?,
            (8, WIRE_LEN) => p.geography1 = r.read_string()?,
            (9, WIRE_LEN) => p.geography2 = r.read_string()?,
            (10, WIRE_LEN) => p.geography3 = r.read_string()?,
            (11, WIRE_LEN) => p.geography4 = r.read_string()?,
            (12, WIRE_LEN) => p.geography5 = r.read_string()?,
            (13, WIRE_LEN) => p.cause34 = r.read_string()?,
            (14, WIRE_LEN) => p.cause_list_number = r.read_string()?,
            (15, WIRE_LEN) => p.icd10 = r.read_string()?,
            (16, WIRE_64BIT) => p.age = Some(r.read_double()?),
            (17, WIRE_VARINT) => p.sex = Some(r.read_varint()? as i32),
            (18, WIRE_LEN) => p.birth_date = r.read_string()?,
            (19, WIRE_LEN) => p.death_date = r.read_string()?,
            (20, WIRE_LEN) => p.interview_date = r.read_string()?,
            (21, WIRE_LEN) => p.cause1 = Some(r.read_string()?),
            (22, WIRE_LEN) => p.likelihood1 = Some(r.read_string()?),
            (23, WIRE_LEN) => p.key_symptom1 = Some(r.read_string()?),
            (24, WIRE_LEN) => p.cause2 = Some(r.read_string()?),
            (25, WIRE_LEN) => p.likelihood2 = Some(r.read_string()?),
            (26, WIRE_LEN) => p.key_symptom2 = Some(r.read_string()?),
            (27, WIRE_LEN) => p.cause3 = Some(r.read_string()?),
            (28, WIRE_LEN) => p.likelihood3 = Some(r.read_string()?),
            (29, WIRE_LEN) => p.key_symptom3 = Some(r.read_string()?),
            (30, WIRE_LEN) => p.all_symptoms = Some(r.read_string()?),
            (_, wt) => r.skip_field(wt)?,
        }
    }
    Ok(p)
}

fn decode_csv_row(bytes: &[u8]) -> Result<Vec<String>, SmartVaError> {
    let mut r = ProtoReader::new(bytes);
    let mut cells = Vec::new();
    while !r.is_eof() {
        let (field, wt) = r.read_tag()?;
        match (field, wt) {
            (1, WIRE_LEN) => cells.push(r.read_string()?),
            (_, wt) => r.skip_field(wt)?,
        }
    }
    Ok(cells)
}

fn decode_csv_table(bytes: &[u8]) -> Result<(String, Vec<Vec<String>>), SmartVaError> {
    let mut r = ProtoReader::new(bytes);
    let mut name = String::new();
    let mut headers = Vec::new();
    let mut data_rows = Vec::new();
    while !r.is_eof() {
        let (field, wt) = r.read_tag()?;
        match (field, wt) {
            (1, WIRE_LEN) => name = r.read_string()?,
            (2, WIRE_LEN) => headers.push(r.read_string()?),
            (3, WIRE_LEN) => data_rows.push(decode_csv_row(r.read_bytes()?)?),
            (_, wt) => r.skip_field(wt)?,
        }
    }
    let mut all_rows = Vec::with_capacity(data_rows.len() + 1);
    if !headers.is_empty() {
        all_rows.push(headers);
    }
    all_rows.extend(data_rows);
    Ok((name, all_rows))
}

fn decode_csmf_entry(bytes: &[u8]) -> Result<(String, f64), SmartVaError> {
    let mut r = ProtoReader::new(bytes);
    let mut k = String::new();
    let mut v = 0.0;
    while !r.is_eof() {
        let (field, wt) = r.read_tag()?;
        match (field, wt) {
            (1, WIRE_LEN) => k = r.read_string()?,
            (2, WIRE_64BIT) => v = r.read_double()?,
            (_, wt) => r.skip_field(wt)?,
        }
    }
    Ok((k, v))
}

/// Decode a Protocol Buffers v3 binary payload (`smartva.SmartVaResult`) back into a
/// [`SmartVaCliDirectoryResult`].
pub fn decode_result_protobuf(bytes: &[u8]) -> Result<SmartVaCliDirectoryResult, SmartVaError> {
    let mut r = ProtoReader::new(bytes);
    let mut res = SmartVaCliDirectoryResult {
        total_vas: 0,
        predictions: Vec::new(),
        csv_tables: BTreeMap::new(),
        csmf_all: BTreeMap::new(),
        csmf_adult: BTreeMap::new(),
        csmf_child: BTreeMap::new(),
        csmf_neonate: BTreeMap::new(),
        csmf_rows: Vec::new(),
        csmf_adult_rows: Vec::new(),
        csmf_child_rows: Vec::new(),
        csmf_neonate_rows: Vec::new(),
        gbd_level1_csmf_rows: Vec::new(),
        causes_of_death_rows: Vec::new(),
        age_groupings_rows: Vec::new(),
        warnings: Vec::new(),
        report_text: String::new(),
        xlsx_files_written: 0,
        png_charts_written: 0,
    };

    while !r.is_eof() {
        let (field, wt) = r.read_tag()?;
        match (field, wt) {
            (1, WIRE_VARINT) => res.total_vas = r.read_varint()? as usize,
            (2, WIRE_LEN) => res.predictions.push(decode_prediction(r.read_bytes()?)?),
            (3, WIRE_LEN) => {
                let (name, rows) = decode_csv_table(r.read_bytes()?)?;
                res.csv_tables.insert(name, rows);
            }
            (4, WIRE_LEN) => res.warnings.push(r.read_string()?),
            (5, WIRE_LEN) => {
                let (k, v) = decode_csmf_entry(r.read_bytes()?)?;
                res.csmf_all.insert(k, v);
            }
            (6, WIRE_LEN) => {
                let (k, v) = decode_csmf_entry(r.read_bytes()?)?;
                res.csmf_adult.insert(k, v);
            }
            (7, WIRE_LEN) => {
                let (k, v) = decode_csmf_entry(r.read_bytes()?)?;
                res.csmf_child.insert(k, v);
            }
            (8, WIRE_LEN) => {
                let (k, v) = decode_csmf_entry(r.read_bytes()?)?;
                res.csmf_neonate.insert(k, v);
            }
            (9, WIRE_LEN) => res.report_text = r.read_string()?,
            (_, wt) => r.skip_field(wt)?,
        }
    }

    if res.total_vas == 0 && !res.predictions.is_empty() {
        res.total_vas = res.predictions.len();
    }

    Ok(res)
}
