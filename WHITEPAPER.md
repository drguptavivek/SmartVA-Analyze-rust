# Technical Whitepaper: `smartva-rust` — A 100% Pure Native Rust Implementation of IHME SmartVA-Analyze (Tariff 2.0)

**Version:** `0.2.0` (`SmartVA-Analyze 3.0.0-rust`)  
**Date:** October 20, 2026  
**Baseline Reference:** Upstream [`ihmeuw/SmartVA-Analyze`](https://github.com/ihmeuw/SmartVA-Analyze) `v3.0.0` (Abraham Flaxman *et al.*, Institute for Health Metrics and Evaluation)

---

## Abstract

Verbal Autopsy (VA) automated cause-of-death assignment via the IHME Tariff 2.0 algorithm (`SmartVA-Analyze`) is a cornerstone of population mortality surveillance across low- and middle-income countries. However, the upstream Python implementation (`ihmeuw/SmartVA-Analyze` v3.0.0) carries heavy runtime dependencies (`numpy`, `scipy`, `matplotlib`, `xlsxwriter`, `pyyaml`, `click`, `progressbar2`), incurs multi-second interpreter and model-training startup overhead on every CLI invocation, and suffers from a Python 3 `io.StringIO.truncate(0)` bug in `UnicodeWriter` that quadratic-pads `{adult,child,neonate}-likelihoods.csv` with megabytes of null bytes (`\x00`).

We present **`smartva-rust` (`smartva-rs` / `libsmartva_rust`)**, a **100% pure native Rust** implementation of the complete `SmartVA-Analyze` v3.0.0 pipeline (`WHOPrep` $\rightarrow$ `CommonPrep` $\rightarrow$ `PreSymptomPrep` $\rightarrow$ `RulesPrep` $\rightarrow$ `SymptomPrep` $\rightarrow$ `TariffPrep` $\rightarrow$ `OutputPrep`, plus `.xlsx` likelihood workbooks and `.png` cohort figures). `smartva-rust` has **zero runtime Python dependency** and **zero JSON conversion of SmartVA data**: all 19 Python domain modules (`src/smartva/data/*.py`) are replaced 1-for-1 with native Rust modules (`rust/src/data/*.rs`), and all 9 SmartVA reference CSVs (`tariffs-*.csv`, `validated-*.csv`, `*_undetermined_weights.csv`) are consumed directly in Rust via the `csv` crate.

Evaluated across **15 anonymized real-world WHO Verbal Autopsy cohorts (`Cohort-01` .. `Cohort-15`, `8,273` VAs total)** with full chart and Excel generation (`--figures True`), `smartva-rust` achieves:
- **100.00% (`8,273 / 8,273`) individual cause-of-death (`cause34`) and ICD-10 parity** with upstream Python `SmartVA-Analyze`.
- **0 cell differences across all 23 output and intermediate CSV files per cohort (`345` CSV files total)**.
- **`9.40x` overall speedup in Cold CLI subprocess mode** (`10.12 s` vs. `95.17 s`, up to **`14.9x`** per cohort).
- **`13.18x` overall speedup in Warm In-Process FFI mode** (`7.22 s` vs. `95.17 s` with `--figures True`, and `6.78 s` / **`14.04x`** with `--figures False`; up to **`41.7x`** faster on small cohorts).
- **`503x` reduction in likelihood CSV disk footprint** on `Cohort-01` (`372 KB` vs. `187.5 MB` in Python) by eliminating the upstream `UnicodeWriter` null-byte corruption bug.

---

## 1. Architecture & 1-for-1 Source Data Management

A core design principle of `smartva-rust` is **structural fidelity to upstream `SmartVA-Analyze`**:
- Where upstream Python stores static domain tables in `src/smartva/data/*.py`, `smartva-rust` stores them 1-for-1 in `rust/src/data/*.rs`.
- Where upstream Python reads reference CSVs (`tariffs-{module}.csv`, `validated-{module}.csv`, `{module}_undetermined_weights.csv`) from `src/smartva/data/*.csv`, `smartva-rust` reads the exact same CSV files directly from `rust/src/data/*.csv` using Rust's `csv::Reader`.

```
Input CSV (WHO 2016 / WHO 2022 / PHMRC Short / PHMRC Full)
   │
   ▼
[1. Header Formatting & Form Detection] (engine.rs)
   │  Strips ODK group prefixes (: / -), detects WHO 2016 vs WHO 2022 vs PHMRC
   ▼
[2. WHOPrep] (engine.rs + data/who_data.rs)
   │  Maps WHO Id10004..Id10481 items, ages, durations, multiselects -> cleanheaders.csv
   ▼
[3. CommonPrep] (engine.rs + data/common_data.rs)
   │  Validates SIDs, consent (gen_3_1), age routing -> {adult,child,neonate}-prepped.csv
   ▼
[4. PreSymptomPrep] (engine.rs + data/*_pre_symptom_data.rs + stemmer.rs)
   │  Recodes categoricals, computes duration/age/WHO weight-for-age SDs, Porter2 free-text stemming
   ▼
[5. RulesPrep] (engine.rs + ldap.rs)
   │  Evaluates 20 deterministic clinical rules & prefix LDAP expressions -> *-logic-rules.csv
   ▼
[6. SymptomPrep] (engine.rs + data/*_symptom_data.rs)
   │  Applies age quartiles, duration cutoffs, injury windows, censored/required cause masks
   ▼
[7. TariffPrep] (engine.rs + data/*_tariff_data.rs + data/tariffs-*.csv + data/validated-*.csv)
   │  Top-40 round-to-0.5 tariff scoring, uniform validation ranking, cause34 reduction, CSMF redistribution
   ▼
[8. OutputPrep + Native XLSX & PNG Renderers] (engine.rs + xlsx.rs + charts.rs)
   ├── 1-individual-cause-of-death/individual-cause-of-death.csv & {module}-predictions.csv
   ├── 2-csmf/{adult,child,neonate}-csmf.csv, csmf.csv, gbd-level1-csmf.csv
   ├── 3-graphs-and-tables/causes-of-death.csv & 9 cohort figures (*.png via plotters)
   └── 4-monitoring-and-quality/{module}-likelihoods.xlsx (via rust_xlsxwriter), report.txt, intermediate-files/
```

---

## 2. Numerical & Algorithmic Precision Guarantees

Achieving **0 CSV cell differences across all 345 output and intermediate CSV files** required exact reproduction of Python 3 / NumPy semantics across seven subtle edge cases:

1. **Banker's Rounding (`round_ties_even`) on Tariff Precision (`0.5`)**:
   In `clean_tariffs`, upstream Python computes `np.round(v / 0.5) * 0.5`. `smartva-rust` preserves exact CSV column order from `tariffs-{module}.csv`, performs a stable sort by `|weight|` descending (`sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()))`), truncates to 40 symptoms, and applies IEEE-754 round-half-to-even (`(v / 0.5).round_ties_even() * 0.5`).
2. **Distinguishing Zero Endorsed Symptoms (`"0"`) vs. Zero-Sum Endorsed Symptoms (`"0.0"`)**:
   In Python's `TariffPrep.score_row`, `sum(tariff for symptom, tariff in symptoms if symptom in endorsements)` returns integer `0` (`"0"`) when the generator is empty, and float `0.0` (`"0.0"`) when $\ge 1$ endorsed symptoms sum to zero. `engine.rs` tracks `(score: f64, had_endorsed: bool)` so `{module}-tariff-scores.csv` matches Python byte-for-byte.
3. **Duration Multiplication Type Formatting (`"0"` vs. `"0.0"`)**:
   In `PreSymptomPrep.calculate_duration_vars`, `TIME_FACTORS.get(code_value, 0) * length_value` yields integer `0` (`"0"`) when `code_value` is outside `1..=6` and a `float` (`"0.0"`) when `code_value` is in `1..=6` and `length_value == 0`.
4. **Midpoint Tie-Rank Averaging (`bisect_left` / `bisect_right`)**:
   External ranking against the ascending `uniform_scores` distribution uses binary-search partition points (`gt = partition_point(|&x| x < s)`, `lt = partition_point(|&x| x <= s)`) to compute `(N - gt) + (gt - lt) / 2.0 + 0.5`.
5. **First-Index Symptom Description Ordering (`symptom_order.index(s)`)**:
   In `neonate_tariff_data.SYMPTOM_DESCRIPTIONS`, `"Difficulty breathing"` appears twice (`s73` and `s76`). Python's `list.index(s)` returns the first index. `engine.rs` populates `symp_order` via `.entry(desc).or_insert(i)` so `all_symptoms` in `{module}-likelihoods.csv` sorts identically.
6. **Relative-Delta Calendar Month Calculation (`dateutil.relativedelta`)**:
   For child weight-for-age SD scoring (`process_weight_sd_vars`), `engine.rs` replicates `dateutil.relativedelta(latest_exam, dob)` month clamping across variable-length months (`days_in_month`).
7. **Elimination of Upstream Python's `UnicodeWriter` Null-Byte (`\x00`) Bug**:
   Upstream Python's `UnicodeWriter.writerow` calls `self.queue.truncate(0)` on an `io.StringIO` without `self.queue.seek(0)`, causing Python 3 to pad `{module}-likelihoods.csv` with $O(N^2)$ null bytes (`187.5 MB` on `Cohort-01`). `smartva-rust` writes clean RFC-4180 UTF-8 CSVs (`372 KB` on `Cohort-01`) with zero null bytes.

---

## 3. Empirical Validation & Benchmark Results Across 15 Cohorts (`8,273` VAs)

All 15 cohorts were benchmarked on Apple Silicon (`macOS arm64`) with `--country IND --hiv True --malaria True --hce True --freetext True --figures True`.

| Cohort | Input VAs | Scored VAs | `cause34` & ICD-10 Match | CSV Cell Diffs (23 Files) | Charts (`Py` / `Rs`) | Upstream Python (`ms`) | Rust Cold CLI (`ms`) | Cold CLI Speedup | Rust Warm FFI (`ms`) | Warm FFI Speedup |
| --- | ---: | ---: | ---: | ---: | :---: | ---: | ---: | ---: | ---: | ---: |
| **Cohort-01** | 1,104 | 1,104 | `1,104 / 1,104` (`100.0%`) | `0` | `9 / 9` | `9,374.6` | `1,655.0` | **`5.66x`** | `1,095.4` | **`8.56x`** |
| **Cohort-02** | 1,077 | 1,077 | `1,077 / 1,077` (`100.0%`) | `0` | `9 / 9` | `8,112.2` | `999.4` | **`8.12x`** | `842.0` | **`9.63x`** |
| **Cohort-03** | 116 | 116 | `116 / 116` (`100.0%`) | `0` | `8 / 8` | `4,174.7` | `315.8` | **`13.22x`** | `151.5` | **`27.56x`** |
| **Cohort-04** | 820 | 820 | `820 / 820` (`100.0%`) | `0` | `9 / 9` | `7,392.8` | `808.4` | **`9.14x`** | `628.2` | **`11.77x`** |
| **Cohort-05** | 1,258 | 1,258 | `1,258 / 1,258` (`100.0%`) | `0` | `8 / 8` | `8,039.5` | `1,129.1` | **`7.12x`** | `958.5` | **`8.39x`** |
| **Cohort-06** | 1,388 | 1,388 | `1,388 / 1,388` (`100.0%`) | `0` | `9 / 9` | `8,994.7` | `1,220.2` | **`7.37x`** | `1,065.7` | **`8.44x`** |
| **Cohort-07** | 1,169 | 1,169 | `1,169 / 1,169` (`100.0%`) | `0` | `9 / 9` | `8,532.9` | `1,016.5` | **`8.39x`** | `861.0` | **`9.91x`** |
| **Cohort-08** | 178 | 178 | `178 / 178` (`100.0%`) | `0` | `7 / 7` | `4,913.8` | `382.0` | **`12.86x`** | `213.2` | **`23.05x`** |
| **Cohort-09** | 59 | 59 | `59 / 59` (`100.0%`) | `0` | `7 / 7` | `3,959.8` | `265.7` | **`14.90x`** | `107.5` | **`36.84x`** |
| **Cohort-10** | 23 | 23 | `23 / 23` (`100.0%`) | `0` | `6 / 6` | `3,439.8` | `244.1` | **`14.09x`** | `82.5` | **`41.69x`** |
| **Cohort-11** | 116 | 116 | `116 / 116` (`100.0%`) | `0` | `8 / 8` | `4,414.2` | `300.8` | **`14.67x`** | `148.0` | **`29.83x`** |
| **Cohort-12** | 256 | 256 | `256 / 256` (`100.0%`) | `0` | `9 / 9` | `6,388.6` | `468.4` | **`13.64x`** | `279.2` | **`22.88x`** |
| **Cohort-13** | 255 | 255 | `255 / 255` (`100.0%`) | `0` | `7 / 7` | `5,452.4` | `461.9` | **`11.80x`** | `283.1` | **`19.26x`** |
| **Cohort-14** | 227 | 227 | `227 / 227` (`100.0%`) | `0` | `9 / 9` | `6,263.1` | `424.5` | **`14.75x`** | `244.6` | **`25.61x`** |
| **Cohort-15** | 227 | 227 | `227 / 227` (`100.0%`) | `0` | `7 / 7` | `5,718.8` | `428.8` | **`13.34x`** | `263.0` | **`21.74x`** |
| **Total / Overall** | **8,273** | **8,273** | **`8,273 / 8,273` (`100.00%`)** | **`0`** | **`121 / 121`** | **`95,171.9`** | **`10,120.6`** | **`9.40x`** | **`7,223.3`** | **`13.18x`** |

---

## 4. Empirical Two-Stage Validation on the IHME PHMRC Gold Standard Dataset (`12,530` VAs)

Using the lossless reverse-mappers [`evidence/phmrc_source_prep.py`](./evidence/phmrc_source_prep.py) (Python) and [`src/phmrc_source_prep.rs`](./src/phmrc_source_prep.rs) (`phmrc-source-prep` Rust CLI), all three GHDx PHMRC Gold Standard datasets (`IHME_PHMRC_VA_DATA_{ADULT,CHILD,NEONATE}_Y2013M09D11_0.csv`, `N = 12,530` clinically validated hospital deaths across `AP`, `Bohol`, `Dar`, `Mexico`, `Pemba`, and `UP`) were converted into ODK PHMRC Full Instrument CSVs and evaluated in two stages:

1. **100.00% Lossless Symptom Matrix Round-Trip Across All 3 Modules**: `CommonPrep -> PreSymptomPrep -> SymptomPrep` on the reverse-mapped PHMRC Adult (`7,839` rows × `341` symptom columns = `2,673,099` cells), Child (`2,059` rows × `133` symptom columns = `273,847` cells), and Neonate (`2,591` rows × `146` symptom columns = `378,286` cells) datasets matches IHME's official `validated-adult.csv`, `validated-child.csv`, and `validated-neonate.csv` with **`0` cell differences (`100.00%` exact match)** across all 3 modules.
2. **Stage 1 (Algorithm Validation — Unmodified Python `smartva.va_cli` vs. Physician Gold Standard)**: Evaluates unmodified upstream Python `smartva.va_cli` (`v3.0.0`) against hospital physician gold-standard diagnoses (`va46` / `va34` / `gs_text34`), benchmarked against **Serina et al. (*BMC Medicine* 2015, 13:291)** (full per-cause tables in [`evidence/tabulations/PHMRC_Gold_Standard_Summary.md`](./evidence/tabulations/PHMRC_Gold_Standard_Summary.md)).
3. **Stage 2 (Port Equivalence — Pure-Rust `smartva-rs` vs. Python `smartva.va_cli`)**: `smartva-rs` matches `smartva.va_cli` on **`12,530 / 12,530` (`100.00%`)** individual cause-of-death (`cause34` & `icd10`) records and **`100.00%` (`0` cell differences)** across all population CSMF tables (`2-csmf/{adult,child,neonate}-csmf.csv` & `2-csmf/csmf.csv`), while completing all 3 datasets in **`5.93s`** vs. **`118.21s`** in Python (**`19.9x` speedup**):

| Metric | Adult (`34` Causes) | Child (`21` Causes) | Neonate (`6` Causes) | Combined PHMRC (`12,530` VAs) |
|---|---:|---:|---:|---:|
| **Total Gold-Standard Records ($N$)** | `7,841` | `2,064` | `2,625` | **`12,530`** |
| **Stage 1: Determined Predictions (`#` / `%`)** | `7,294` (`93.02%`) | `1,941` (`94.04%`) | `2,429` (`92.53%`) | **`11,664` (`93.09%`)** |
| **Stage 1: Undetermined / No Specific Cause (`#` / `%`)** | `547` (`6.98%`) | `123` (`5.96%`) | `196` (`7.47%`) | **`866` (`6.91%`)** |
| **Stage 1: Top-1 Agreement vs. Physician Gold Standard (Determined)** | `3,692 / 7,294` (**`50.62%`**) | `959 / 1,941` (**`49.41%`**) | `1,685 / 2,429` (**`69.37%`**) | **`6,336 / 11,664` (`54.32%`)** |
| **Stage 1: Top-1 Agreement vs. Physician Gold Standard (All $N$)** | `3,692 / 7,841` (`47.09%`) | `959 / 2,064` (`46.46%`) | `1,685 / 2,625` (`64.19%`) | **`6,336 / 12,530` (`50.57%`)** |
| **Stage 1: Top-3 Agreement vs. Physician Gold Standard (Determined)** | `4,904 / 7,294` (**`67.23%`**) | `1,253 / 1,941` (**`64.55%`**) | `1,941 / 2,429` (**`79.91%`**) | **`8,098 / 11,664` (`69.43%`)** |
| **Stage 1: Mean Cause Sensitivity (on Determined)** | `51.37%` | `58.23%` | `58.66%` | — |
| **Stage 1: Mean Cause Specificity (on Determined)** | `98.43%` | `97.42%` | `93.87%` | — |
| **Stage 1: Mean Positive Predictive Value (PPV)** | `50.21%` | `50.47%` | `57.47%` | — |
| **Stage 1: Mean Chance-Corrected Concordance ($CCC$, Determined) [Serina 2015]** | **`49.89%`** [`50.5%`] | **`56.14%`** [`52.5%`] | **`50.39%`** [`48.0%`] | — |
| **Stage 1: CSMF Accuracy (vs. Gold-Standard CSMF) [Serina 2015]** | **`0.8443`** [`0.770`] | **`0.7937`** [`0.783`] | **`0.8331`** [`0.826`] | — |
| **Stage 2: Individual `cause34` & `icd10` Agreement (`smartva-rs` vs. `va_cli`)** | `7,841 / 7,841` (**`100.00%`**) | `2,064 / 2,064` (**`100.00%`**) | `2,625 / 2,625` (**`100.00%`**) | **`12,530 / 12,530` (`100.00%`)** |
| **Stage 2: Population CSMF Table Diff (`2-csmf/*.csv`: `all`, `male`, `female`)** | **`0` cell diffs (`0.0000`)** | **`0` cell diffs (`0.0000`)** | **`0` cell diffs (`0.0000`)** | **`0` cell diffs (`0.0000`)** |
| **Runtime (`smartva.va_cli` Python vs. `smartva-rs` Rust)** | `89.17s` vs. **`4.48s`** (`19.9x`) | `15.42s` vs. **`0.68s`** (`22.7x`) | `13.62s` vs. **`0.77s`** (`17.7x`) | `118.21s` vs. **`5.93s` (`19.9x`)** |

---

## 5. Known Limitations

1. **English Output Tables & Figures**: Output labels are generated in English (`--language english`).
2. **v2/v3 4-Folder Directory Hierarchy**: `--legacy-format` (v1.1 flat output folder) is accepted for CLI compatibility while writing the standard 4-folder output structure.

---

## 6. Conclusion

`smartva-rust` demonstrates that the complete IHME SmartVA-Analyze (Tariff 2.0) pipeline — including PHMRC Full/Shortened and WHO 2016/2022 questionnaire mapping, Porter2 free-text stemming, prefix LDAP logic rules, uniform validation ranking, CSMF undetermined redistribution, formatted Excel likelihood workbooks, cohort mortality figures, and in-memory JSON/Protobuf serialization — can be executed in **100% pure native Rust** with **zero runtime Python dependency**, **100.00% mathematical equivalence across 20,813 validated VAs (`12,530` PHMRC Gold Standard + `8,273` WHO 2022 + `10` reference examples)**, and an **order-of-magnitude reduction in execution time, memory footprint, and binary size**.

---

## 7. Credits, Acknowledgments & References

- **Upstream SmartVA-Analyze & Tariff 2.0 Method**: Developed by the **Institute for Health Metrics and Evaluation (IHME), University of Washington** ([`ihmeuw/SmartVA-Analyze`](https://github.com/ihmeuw/SmartVA-Analyze), MIT License, Copyright © 2018 University of Washington), led by Peter Serina, Ian Riley, Andrea Stewart, Spencer L. James, Abraham D. Flaxman, Rafael Lozano, Bernardo Hernandez, Christopher J.L. Murray, Alan D. Lopez, and the IHME Verbal Autopsy team.
- **PHMRC Gold Standard Verbal Autopsy Dataset**: Population Health Metrics Research Consortium (PHMRC) investigators and clinical sites (`AP`, `Bohol`, `Dar`, `Mexico`, `Pemba`, `UP`).
- **WHO Verbal Autopsy Standards**: World Health Organization (WHO 2016 and WHO 2022 Verbal Autopsy instruments).
- **Pure-Rust Port (`SmartVA-Rust` / `smartva-rs`)**: Developed and maintained by **Dr. Vivek Gupta** ([`@drguptavivek`](https://github.com/drguptavivek)), All India Institute of Medical Sciences (AIIMS), New Delhi.

### References
1. Serina P, Riley I, Stewart A, James SL, Flaxman AD, Lozano R, Hernandez B, et al. *Improving performance of the Tariff Method for assigning causes of death to verbal autopsies.* **BMC Medicine**. 2015;13:291. doi:[10.1186/s12916-015-0527-9](https://doi.org/10.1186/s12916-015-0527-9)
2. Serina P, Riley I, Stewart A, et al. *A shortened verbal autopsy instrument for use in routine mortality surveillance systems.* **BMC Medicine**. 2015;13:302. doi:[10.1186/s12916-015-0528-8](https://doi.org/10.1186/s12916-015-0528-8)
3. Murray CJL, Lopez AD, Black R, et al. *Population Health Metrics Research Consortium gold standard verbal autopsy validation study: design, implementation, and development of analysis datasets.* **Population Health Metrics**. 2011;9:27. doi:[10.1186/1478-7954-9-27](https://doi.org/10.1186/1478-7954-9-27)
4. James SL, Flaxman AD, Murray CJL; Population Health Metrics Research Consortium (PHMRC). *Performance of the Tariff Method: validation of a simple additive algorithm for analysis of verbal autopsies.* **Population Health Metrics**. 2011;9:31. doi:[10.1186/1478-7954-9-31](https://doi.org/10.1186/1478-7954-9-31)
