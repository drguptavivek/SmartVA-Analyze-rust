# `smartva-rust` (`smartva-rs`) — 100% Pure Native Rust SmartVA-Analyze Engine

`smartva-rust` is a **100% pure native Rust** implementation of the [IHME SmartVA-Analyze (Tariff 2.0)](https://github.com/ihmeuw/SmartVA-Analyze) Verbal Autopsy cause-of-death assignment pipeline (`v3.0.0`).

It provides:
1. **Drop-In Native CLI (`smartva-rs`)** — 100% CLI-flag compatible with upstream Python `smartva` (`va_cli.py`), writing the identical 4-folder output hierarchy (`1-individual-cause-of-death/`, `2-csmf/`, `3-graphs-and-tables/`, `4-monitoring-and-quality/`) plus optional in-memory JSON and Protocol Buffers v3 output (`--format json`, `--format protobuf`).
2. **Direct Native Rust Library Crate (`smartva_rust::SmartVaEngine`)** — score CSV files or in-memory JSON/CSV payloads directly in Rust with **zero Python dependency**.
3. **C-ABI Shared Library (`libsmartva_rust.dylib` / `.so` / `.dll`)** — callable in-process from C, C++, Python (`ctypes` / `cffi`), R, Go, or Swift with cached Tariff 2.0 validation matrices (`smartva_warm_cache`).
4. **PHMRC Gold Standard Reverse-Mapper (`phmrc-source-prep`)** — lossless preprocessor converting GHDx PHMRC Gold Standard CSV datasets (`IHME_PHMRC_VA_DATA_{ADULT,CHILD,NEONATE}_Y2013M09D11_0.csv`) into ODK PHMRC Full Instrument CSVs for end-to-end validation.

---

## Getting Started

### 1. Build or Download Compiled Binaries

#### Option A: Download Prebuilt Binaries (GitHub Releases)
Prebuilt release archives for Linux (`x86_64`, `aarch64`), macOS (`arm64` Apple Silicon, `x86_64`), and Windows (`x86_64`) are published on the [GitHub Releases](https://github.com/drguptavivek/SmartVA-Analyze-rust/releases) page.

#### Option B: Build from Source with Cargo
Requires a standard Rust toolchain (`rustup`, Rust 1.80+):

```bash
git clone https://github.com/drguptavivek/SmartVA-Analyze-rust.git
cd SmartVA-Analyze-rust
cargo build --release --bins --lib
```

### 2. Where Compiled Binaries & Libraries Are Located

After running `cargo build --release --bins --lib`, all artifacts are placed in `target/release/`:

| Artifact Path | Size | Description |
|---|---:|---|
| `target/release/smartva-rs` (`smartva-rs.exe` on Windows) | `~3.1 MB` | Standalone drop-in CLI executable (all 9 Tariff 2.0 reference CSVs embedded in binary) |
| `target/release/phmrc-source-prep` | `~0.4 MB` | Standalone CLI converting GHDx PHMRC Gold Standard CSVs to ODK PHMRC Full Instrument CSVs |
| `target/release/libsmartva_rust.dylib` (macOS) / `libsmartva_rust.so` (Linux) / `smartva_rust.dll` (Windows) | `~2.6 MB` | C-ABI dynamic shared library for in-process FFI calls (`smartva_warm_cache`, `smartva_run_cli`, `smartva_score_json`) |
| `target/release/libsmartva_rust.rlib` | `~10.9 MB` | Rust static library artifact for Rust workspace integration |

---

### 3. Quick Start: Standalone CLI (`smartva-rs`)

```bash
# A. Standard 4-folder directory output (drop-in replacement for `smartva` / `python -m smartva.va_cli`)
./target/release/smartva-rs \
  --country IND \
  --hiv True \
  --malaria True \
  --hce True \
  --freetext True \
  /path/to/input_va.csv \
  /path/to/output_dir

# B. Score a single VA record from JSON in memory and print individual cause + likelihoods as JSON
./target/release/smartva-rs \
  --country IND \
  --json '{"sid":"VA-001","Id10013":"yes","Id10019":"male","ageInYears":62,"Id10147":"yes"}' \
  --format json

# C. Score a cohort CSV in memory and return extended JSON (includes population CSMF tables & report)
./target/release/smartva-rs \
  --country IND \
  --format json-extended \
  /path/to/input_va.csv

# D. Score a cohort CSV and write Protocol Buffers (proto3 wire format, see proto/smartva.proto)
./target/release/smartva-rs \
  --country IND \
  --format protobuf \
  --output-file result.pb \
  /path/to/input_va.csv

# E. Fast CSV-only directory run (skip PNG chart rendering)
./target/release/smartva-rs --no-charts /path/to/input_va.csv /path/to/output_dir

# F. List all 188 supported ISO-3 country codes
./target/release/smartva-rs --list-countries
```

---

### 4. Quick Start: Rust Crate (`smartva_rust`)

Add `smartva-rust` to your `Cargo.toml`:

```toml
[dependencies]
smartva-rust = { git = "https://github.com/drguptavivek/SmartVA-Analyze-rust.git" }
```

Use `SmartVaEngine` directly in Rust:

```rust
use std::path::Path;
use smartva_rust::{SmartVaEngine, SmartVaRunOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = SmartVaEngine::new();

    // Optional: pre-warm the Adult, Child, and Neonate Tariff 2.0 validation matrices (~167 ms once)
    engine.warm_cache()?;

    let options = SmartVaRunOptions {
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
    };

    // Option A: Full 4-folder directory output
    let result = engine.run_cli_directory(
        Path::new("input_va.csv"),
        Path::new("smartva_output"),
        &options,
    )?;
    println!("Scored {} VAs", result.total_vas);

    // Option B: Score JSON payload in memory (no disk I/O)
    let json_out = engine.score_json(
        r#"{"sid":"VA-001","Id10013":"yes","Id10019":"male","ageInYears":62,"Id10147":"yes"}"#,
        &options,
    )?;
    println!("JSON output: {}", json_out);
    Ok(())
}
```

---

### 5. Quick Start: In-Process C-ABI FFI (Python / C / C++)

External applications can load `libsmartva_rust.{dylib,so,dll}` in-process to keep the 30,000-draw Tariff 2.0 validation matrices warm in memory across calls (`~4–12 ms` per single VA). See [`examples/call_smartva_rs.py`](./examples/call_smartva_rs.py) for a complete runnable example:

```python
import ctypes, json

lib = ctypes.CDLL("./target/release/libsmartva_rust.dylib")  # or .so / .dll
lib.smartva_warm_cache.restype = ctypes.c_int
lib.smartva_score_json.argtypes = [
    ctypes.c_char_p, ctypes.c_char_p,
    ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
]
lib.smartva_score_json.restype = ctypes.c_void_p
lib.smartva_free_string.argtypes = [ctypes.c_void_p]

lib.smartva_warm_cache()  # Warm 30,000-draw validation matrices once (~167 ms)

payload = json.dumps({"sid": "VA-001", "Id10013": "yes", "Id10019": "male", "ageInYears": 62, "Id10147": "yes"}).encode()
ptr = lib.smartva_score_json(payload, b"IND", 1, 1, 1, 1, 0)
result = json.loads(ctypes.cast(ptr, ctypes.c_char_p).value.decode())
lib.smartva_free_string(ptr)
print(result["individual_cause_of_death"][0])
```

---

## Performance, Memory Footprint & Empirical Validation Summary

### Speed & Memory Comparison (`smartva-rs` vs. Upstream Python `smartva.va_cli`)

| Workload / Metric | Upstream Python `smartva.va_cli` | Pure-Rust `smartva-rs` | Speedup / Memory Reduction |
|---|---:|---:|---:|
| **Runtime Footprint on Disk** | `~250+ MB` (Python + scientific stack) | **`3.1 MB`** single self-contained binary | **`~80x` smaller** |
| **Single VA / Example (`N = 2`, `--no-charts`)** | `1.65s` / `98.6 MB` peak footprint | **`0.07s`** / **`9.4 MB`** peak footprint | **`23.6x` faster / `10.5x` less RAM** |
| **Single VA / Example (`N = 2`, full charts + `.xlsx`)** | `1.65s` / `98.6 MB` peak footprint | **`0.18s`** / **`12.1 MB`** peak footprint | **`9.2x` faster / `8.1x` less RAM** |
| **Medium WHO 2022 Cohort (`N = 256`, `Cohort-02`)** | `1.89s` / `138.4 MB` peak footprint | **`0.22s`** cold (`0.08s` warm) / **`24.8 MB`** | **`8.6x–23.6x` faster / `5.6x` less RAM** |
| **Large WHO 2022 Cohort (`N = 1,104`, `Cohort-12`)** | `3.88s` / `246.2 MB` peak footprint | **`0.49s`** cold (`0.31s` warm) / **`68.5 MB`** | **`7.9x–12.5x` faster / `3.6x` less RAM** |
| **15 WHO 2022 Cohorts Combined (`N = 8,273` VAs)** | `24.22s` total | **`4.19s`** cold / **`2.18s`** warm total | **`5.8x` cold / `11.1x` warm faster** |
| **PHMRC Gold Standard Child (`N = 2,064` VAs)** | `15.42s` / `312.0 MB` peak footprint | **`0.68s`** / **`129.5 MB`** peak footprint | **`22.7x` faster / `2.4x` less RAM** |
| **PHMRC Gold Standard Neonate (`N = 2,625` VAs)** | `13.62s` / `328.4 MB` peak footprint | **`0.77s`** / **`139.1 MB`** peak footprint | **`17.7x` faster / `2.4x` less RAM** |
| **PHMRC Gold Standard Adult (`N = 7,841` VAs)** | `89.17s` | **`4.48s`** / `804.7 MB` peak footprint | **`19.9x` faster** |
| **All 3 PHMRC Gold Standard Modules (`N = 12,530` VAs)** | `118.21s` total | **`5.93s`** total | **`19.9x` faster** |

### Empirical Validation Across 20,813 Verbal Autopsies (All 4 Questionnaire Families)

`smartva-rs` has been empirically validated against unmodified upstream Python `smartva.va_cli` (`v3.0.0`) across **20,813 Verbal Autopsy records** spanning all four questionnaire formats supported by SmartVA:

1. **IHME PHMRC Gold Standard Dataset (`N = 12,530` VAs — `PHMRC Full Questionnaire`)**:
   - **100.00% (`12,530 / 12,530`) Python-vs-Rust Cause Agreement**: `7,841 / 7,841` Adult, `2,064 / 2,064` Child, and `2,625 / 2,625` Neonate records match `smartva.va_cli` identically (`0` cell differences across intermediate symptom matrices `validated-adult.csv` [`2,673,099` cells] and `validated-child.csv` [`273,847` cells]).
   - **Agreement Against PHMRC Hospital Physician Gold-Standard Diagnoses (`va46` / `va34` / `gs_text34`)** (matching published Tariff 2.0 benchmarks in Serina et al., *BMC Medicine* 2015; see [`evidence/tabulations/PHMRC_Gold_Standard_Summary.md`](./evidence/tabulations/PHMRC_Gold_Standard_Summary.md)):

| PHMRC Module | Total Gold-Standard VAs ($N$) | Determined Predictions (`#` / `%`) | Undetermined (`#` / `%`) | Python vs. Rust Parity | Top-1 Agreement on Determined (`%`) | Top-3 Agreement on Determined (`%`) | Mean Sensitivity (Det.) | Mean Specificity (Det.) | Mean $CCC$ (Det.) | CSMF Accuracy |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| **Adult (`34` Causes)** | `7,841` | `7,294` (`93.02%`) | `547` (`6.98%`) | **`100.00%`** (`7,841/7,841`) | **`50.62%`** (`3,692/7,294`) | **`67.23%`** (`4,904/7,294`) | `51.37%` | `98.43%` | **`49.89%`** | **`0.8443`** |
| **Child (`21` Causes)** | `2,064` | `1,941` (`94.04%`) | `123` (`5.96%`) | **`100.00%`** (`2,064/2,064`) | **`49.41%`** (`959/1,941`) | **`64.55%`** (`1,253/1,941`) | `58.23%` | `97.42%` | **`56.14%`** | **`0.7937`** |
| **Neonate (`6` Causes)** | `2,625` | `2,392` (`91.12%`) | `233` (`8.88%`) | **`100.00%`** (`2,625/2,625`) | **`70.19%`** (`1,679/2,392`) | **`80.31%`** (`1,921/2,392`) | `58.95%` | `94.00%` | **`50.74%`** | **`0.8470`** |
| **Combined PHMRC** | **`12,530`** | **`11,627` (`92.79%`)** | **`903` (`7.21%`)** | **`100.00%` (`12,530/12,530`)** | **`54.44%` (`6,330/11,627`)** | **`69.48%` (`8,078/11,627`)** | — | — | — | — |

2. **15 Real-World WHO 2022 Cohorts (`N = 8,273` VAs — `WHO 2022 Questionnaire`)**:
   - **100.00% (`8,273 / 8,273`)** cause-of-death and ICD-10 agreement and **`0` cell differences** across all `345` generated output and intermediate CSV files (`23` CSV files × `15` cohorts).
3. **PHMRC Shortened (`PHMRC_short_example.csv`), PHMRC Full (`example_1.csv`), and WHO 2016 v1.5.1 (`WHO_2016_1_5_1_example.csv`)**:
   - **100.00%** prediction, CSMF, and intermediate CSV parity against upstream Python `smartva.va_cli`.

---

## Architectural Highlights

- **Zero Python Dependency at Runtime**: Neither `pyo3` nor a Python interpreter is linked or required.
- **Zero JSON Conversion of SmartVA Data**: Source data is managed in Rust in the exact same structure as upstream Python:
  - All 19 Python domain modules in `src/smartva/data/*.py` (`who_data.py`, `common_data.py`, `adult_pre_symptom_data.py`, `child_pre_symptom_data.py`, `neonate_pre_symptom_data.py`, `adult_symptom_data.py`, `child_symptom_data.py`, `neonate_symptom_data.py`, `adult_tariff_data.py`, `child_tariff_data.py`, `neonate_tariff_data.py`, `codebook.py`, `answer_ranges.py`, `word_conversions.py`, `default_fill_data.py`, `icds.py`, `gbd_causes.py`, `countries.py`, `constants.py`) are replaced 1-for-1 with compiled native Rust modules in [`src/data/`](./src/data/).
  - All 9 SmartVA reference CSV files (`tariffs-{adult,child,neonate}.csv`, `validated-{adult,child,neonate}.csv`, `{adult,child,neonate}_undetermined_weights.csv`) sit in [`src/data/`](./src/data/) and are consumed directly in Rust via the [`csv`](https://crates.io/crates/csv) crate.
- **Native `.xlsx` & `.png` Generation**:
  - Generates `4-monitoring-and-quality/{adult,child,neonate}-likelihoods.xlsx` natively via [`rust_xlsxwriter`](https://crates.io/crates/rust_xlsxwriter) ([`src/xlsx.rs`](./src/xlsx.rs)).
  - Generates all 9 cohort figures in `3-graphs-and-tables/*.png` (`all-figure.png`, `undetermined-figure.png`, `adult-female-csmf-figure.png`, `adult-male-csmf-figure.png`, `{adult,child,neonate}-csmf-figure.png`, `all-csmf-figure.png`, `gbd-level1-csmf.png`) natively via [`plotters`](https://crates.io/crates/plotters) ([`src/charts.rs`](./src/charts.rs)).

---

## Directory Layout

```text
.
├── Cargo.toml                  # Pure-Rust crate manifest (no pyo3, no serde_json)
├── LICENSE                     # MIT License (University of Washington & contributors)
├── README.md                   # Getting started, CLI/Rust/FFI guide, benchmarks & validation
├── WHITEPAPER.md               # Technical whitepaper, PHMRC Gold Standard & 15-cohort analysis
├── OPTIMIZATIONS.md            # Detailed technical catalog of engine optimizations
├── EVIDENCE.md                 # PHMRC Gold Standard (12,530 VAs) & WHO 2022 (8,273 VAs) evidence
├── proto/
│   └── smartva.proto           # Protocol Buffers v3 wire schema for --format protobuf
├── examples/
│   └── call_smartva_rs.py      # Example Python script invoking smartva-rs CLI & C-ABI FFI
├── evidence/
│   ├── phmrc_source_prep.py    # Standalone Python PHMRC Gold Standard -> ODK CSV preprocessor
│   ├── evaluate_phmrc_gold_standard.py # PHMRC Gold Standard accuracy/sens/spec/CCC/CSMF evaluator
│   ├── metrics/
│   │   ├── phmrc_gold_standard_validation.json
│   │   └── upstream_vs_smartva_rs_15cohorts.json
│   ├── tabulations/
│   │   ├── PHMRC_Gold_Standard_Summary.md
│   │   └── Cohort-01..15_Summary.md
│   └── charts/
│       └── Cohort-01..15/{python,rust}/*.png
└── src/
    ├── lib.rs                  # Public Rust API (SmartVaEngine) & C-ABI FFI exports
    ├── engine.rs               # Full pipeline (WHOPrep -> CommonPrep -> PreSymptomPrep -> RulesPrep -> SymptomPrep -> TariffPrep -> OutputPrep)
    ├── phmrc_source_prep.rs    # Native Rust PHMRC Gold Standard -> ODK CSV preprocessor
    ├── ldap.rs                 # Pure-Rust LdapNotationParser & 20 deterministic clinical rules
    ├── stemmer.rs              # Pure-Rust Porter2 English stemmer (free-text symptom extraction)
    ├── xlsx.rs                 # Native rust_xlsxwriter .xlsx likelihood workbook generator
    ├── charts.rs               # Native plotters .png CSMF & age-distribution chart generator
    ├── bin/
    │   ├── smartva_rs.rs       # Drop-in CLI binary matching va_cli.py flags
    │   └── phmrc_source_prep.rs# CLI binary for PHMRC Gold Standard CSV preprocessing
    └── data/                   # 1-for-1 Rust modules & embedded Tariff 2.0 reference CSVs
```

---

## Known Limitations

- **English Output Labels Only**: `--language` currently supports English output tables/charts (Chinese and Spanish translation tables from upstream `smartva` are not bundled).
- **v2+ Output Directory Layout**: `--legacy-format` (SmartVA v1.1 flat directory layout) is accepted for CLI flag compatibility, while file output uses the standard SmartVA v2/v3 4-folder hierarchy.

---

## License & Upstream Acknowledgment

Released under the **MIT License** (see [`LICENSE`](./LICENSE)).

This project is a pure-Rust reimplementation of [**SmartVA-Analyze**](https://github.com/ihmeuw/SmartVA-Analyze), originally developed in Python by the **Institute for Health Metrics and Evaluation (IHME), University of Washington** (Copyright (c) 2018 University of Washington) and implementing the **Tariff 2.0** verbal autopsy method described in:
- Serina P, Riley I, Stewart A, et al. *Improving performance of the Tariff Method for assigning causes of death to verbal autopsies.* **BMC Medicine** 2015; 13:291.
