# Technical Catalog of Optimizations in `smartva-rust` (`v0.2.0`)

This document details the engineering and algorithmic optimizations implemented in `smartva-rust` (`smartva-rs` / `libsmartva_rust`) relative to upstream Python `ihmeuw/SmartVA-Analyze` (`v3.0.0`), while preserving **100.00% mathematical and CSV cell-level parity** (`8,273 / 8,273` VAs and `0` CSV cell diffs across all 23 output/intermediate CSV files $\times$ 15 cohorts).

---

## Summary of Empirical Performance Gains (15 Cohorts, `8,273` VAs)

| Execution Mode | Total Time across 15 Cohorts (`8,273` VAs) | Mean Throughput | Speedup vs. Upstream Python (`--figures True`) |
| --- | ---: | ---: | ---: |
| **Upstream Python (`smartva.va_cli --figures True`)** | `95,171.92 ms` (`95.17 s`) | `86.9 VAs/sec` | `1.00x` (baseline) |
| **Pure-Rust Cold CLI (`smartva-rs --figures True`)** | `10,120.58 ms` (`10.12 s`) | `817.4 VAs/sec` | **`9.40x`** (up to **`14.9x`** per cohort) |
| **Pure-Rust Cold CLI (`smartva-rs --figures False`)** | `9,363.16 ms` (`9.36 s`) | `883.6 VAs/sec` | **`10.16x`** |
| **Pure-Rust Warm FFI (`libsmartva_rust --figures True`)** | `7,223.29 ms` (`7.22 s`) | `1,145.3 VAs/sec` | **`13.18x`** (up to **`41.7x`** per cohort) |
| **Pure-Rust Warm FFI (`libsmartva_rust --figures False`)** | `6,779.04 ms` (`6.78 s`) | `1,220.4 VAs/sec` | **`14.04x`** |

---

## 1. Zero-Serialization Static Domain Tables (`rust/src/data/*.rs`)

### Upstream Python Bottleneck
Every invocation of `python -m smartva.va_cli` imports `numpy`, `scipy`, `matplotlib`, `xlsxwriter`, `click`, `progressbar`, `pyyaml`, and 19 Python data modules in `src/smartva/data/*.py` containing thousands of nested dictionaries, tuples, and lists (`who_data.py`, `common_data.py`, `adult_pre_symptom_data.py`, `adult_symptom_data.py`, `adult_tariff_data.py`, `codebook.py`, etc.). Constructing these Python heap objects and initializing `matplotlib`'s font manager consumes `~1.8–2.5 seconds` before a single VA row is read.

### Pure-Rust Optimization
All 19 Python data modules in `src/smartva/data/*.py` are compiled 1-for-1 into static Rust `.rodata` slices (`&'static [...]`) in `rust/src/data/*.rs`:
- Zero runtime parsing or JSON deserialization.
- Placed in read-only memory pages shared across processes by the OS kernel.
- Process startup time drops from `~2,000 ms` to `< 3 ms`.

---

## 2. Direct CSV Streaming & Bit-Indexed Tariff Validation Matrix Scoring (`engine.rs`)

### Upstream Python Bottleneck
In upstream `TariffPrep.run()` (`src/smartva/tariff_prep.py`), every invocation:
1. Reads `tariffs-{adult,child,neonate}.csv` and `validated-{adult,child,neonate}.csv` (`7,594` validated VAs $\times$ `~250` symptom columns) via `csv.DictReader`, allocating a Python `dict` of `str -> str` per row.
2. Converts each validated row into a `set` of endorsed symptom strings (`endorsements`) and computes `sum(tariff for symptom, tariff in symptoms if symptom in endorsements)` across 46 causes using string hashing in nested Python loops.
3. Expands `uniform_train` (`~16,000–23,000`ampled references per module) and sorts it 46 times in Python (`sorted(uniform_train, key=lambda va: (-va.scores[cause], va.sid))`).

### Pure-Rust Optimization
In `rust/src/engine.rs` (`get_or_build_trained_model`):
1. Reads `src/data/tariffs-{module}.csv` and `src/data/validated-{module}.csv` directly via `csv::ReaderBuilder` without constructing per-row `HashMap<String, String>`.
2. Maps each tariff's top-40 `(symptom_name, weight)` pairs once to integer column indices `(col_idx: usize, weight: f64)` in `validated-{module}.csv`.
3. Evaluates each validated VA row using a contiguous stack/vec boolean slice (`endorsed: Vec<bool>`) indexed by `col_idx` — replacing hundreds of thousands of Python string hash lookups with direct $O(1)$ array indexing.
4. Caches the resulting `Arc<TrainedModuleModel>` in a thread-safe `OnceLock<Mutex<HashMap<TrainingCacheKey, Arc<TrainedModuleModel>>>>`:
   - Cold CLI builds all 3 module models in **`167 ms`** (vs. `~2,800 ms` in Python — **`16.7x` faster**).
   - Warm FFI (`smartva_warm_cache`) reuses the trained models in **`0.00 ms`** across all subsequent runs.

---

## 3. $O(\log M)$ Binary-Search External Ranking (`partition_point`)

### Upstream Python Equivalence & Speedup
When ranking user VAs against the uniform validation distribution (`uniform_scores[cause]`, sorted ascending of length $M \approx 23,000$), `engine.rs` uses Rust's slice binary search (`u_scores.partition_point(|&x| x < s)` and `u_scores.partition_point(|&x| x <= s)`) over contiguous `&[f64]` slices, computing exact midpoint tie-averaged ranks in $O(\log M)$ with L1-cache-resident `f64` comparisons.

---

## 4. Elimination of Upstream Python's $O(N^2)$ Null-Byte (`\x00`) Bug in `UnicodeWriter`

### Upstream Python Bug Discovered During Verification
In `src/smartva/utils/utils.py` (lines 141–147), upstream `SmartVA-Analyze` defines `UnicodeWriter.writerow`:
```python
def writerow(self, row):
    self.writer.writerow([s for s in row])
    data = self.queue.getvalue()
    self.stream.write(data)
    self.queue.truncate(0)  # BUG in Python 3: does not reset stream position to 0!
```
In Python 3's `io.StringIO`, `truncate(0)` truncates the buffer without resetting the seek position (`seek(0)`). Consequently, on row $k$, `StringIO` zero-fills all bytes from offset `0` to the cumulative byte offset of rows $1 \dots k-1$ with **null bytes (`\x00`)** before appending row $k$.
- On `Cohort-01` (`993` adult VAs), upstream Python writes **`187,476,773` bytes (`187.5 MB`)** into `adult-likelihoods.csv`, of which **`99.80%` (`187.1 MB`) are corrupt `\x00` null bytes**!
- In `smartva-rust`, all CSV writers use `csv::WriterBuilder` over `BufWriter<File>`, writing a clean **`372,103` bytes (`372 KB`)** — a **`503x` reduction in file size and disk I/O** on `Cohort-01` while producing 100% identical non-null CSV content.

---

## 5. Native Rust `.xlsx` Generation (`rust_xlsxwriter`) & `.png` Charting (`plotters`)

- **`.xlsx` Workbooks (`src/xlsx.rs`)**: Uses `rust_xlsxwriter` to write `4-monitoring-and-quality/{adult,child,neonate}-likelihoods.xlsx` with identical column widths, frozen panes, row heights (`52.50 pt`), vertical alignment, and conditional RGB likelihood fills (`#3CB371`, `#47D147`, `#8AE600`, `#E6E600`) in `< 15 ms` per cohort.
- **`.png` Figures (`src/charts.rs`)**: Uses `plotters` + `ab_glyph` to render all 9 SmartVA figures (`all-figure.png`, `undetermined-figure.png`, `adult-female-csmf-figure.png`, `adult-male-csmf-figure.png`, `{adult,child,neonate}-csmf-figure.png`, `all-csmf-figure.png`, `gbd-level1-csmf.png`) natively in Rust in `~30–50 ms` per cohort (vs. `1,500–2,500 ms` in Python `matplotlib`).

---

## 6. Pure-Rust Porter2 Stemmer & Recursive-Descent LDAP Rule Parser

- **Porter2 Stemmer (`src/stemmer.rs`)**: Replaces the Python `stemming.porter2` library with a zero-allocation-heavy state machine implementing the full English Porter2 specification (`step_1a`..`step_5`, `R1`/`R2` regions, special word table).
- **LDAP Expression & Clinical Rules Engine (`src/ldap.rs`)**: Replaces Python's `LdapNotationParser` and dynamic `rules_prep.py` evaluation with a compiled recursive-descent prefix parser (`eval_ldap`) and native Rust clinical rule evaluators (`evaluate_module_rules`).
