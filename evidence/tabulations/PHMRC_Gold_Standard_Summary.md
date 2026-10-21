# PHMRC Gold Standard Validation & Python-vs-Rust Parity Report

## 1. Executive Summary

This report documents the empirical validation of **SmartVA-Analyze** — both the unmodified upstream Python reference CLI (`smartva.va_cli`) and the Pure-Rust implementation (`smartva-rs`) — across the complete **IHME Population Health Metrics Research Consortium (PHMRC) Gold Standard Verbal Autopsy Dataset (`Y2013M09D11_0`)** comprising **12,530 clinically validated verbal autopsies** across 6 international sites (`AP`, `Bohol`, `Dar`, `Mexico`, `Pemba`, `UP`):

- **Adult Module (`N = 7,841`)**: 34 target reporting causes (reduced from 46 internal tariff causes).
- **Child Module (`N = 2,064`)**: 21 target reporting causes.
- **Neonate Module (`N = 2,625`)**: 6 target reporting causes (including stillbirths).

### Key Validation Findings

1. **100.00% Lossless Symptom Matrix Round-Trip**: Reverse-mapping the GHDx PHMRC Gold Standard CSV files into ODK PHMRC Full Instrument CSVs (`evidence/phmrc_source_prep.py` and `src/phmrc_source_prep.rs`) and running SmartVA's `CommonPrep -> PreSymptomPrep -> SymptomPrep` pipeline reproduces IHME's official `test/integration_data/validated-adult.csv` (**341 symptom columns × 7,839 rows = 2,673,099 cells**) and `validated-child.csv` (**133 symptom columns × 2,059 rows = 273,847 cells**) with **0 cell differences (`100.00%` exact match)**.
2. **100.00% Exact Parity Between Python (`smartva.va_cli`) and Rust (`smartva-rs`)**: Across all **12,530 / 12,530 (`100.00%`)** PHMRC Gold Standard records, `smartva-rs` produces the **exact same primary cause of death (`cause34`) and ICD-10 code (`icd10`)** as unmodified upstream Python `smartva.va_cli`, as well as 100% matching CSMF tables (`2-csmf/*.csv`) and intermediate symptom matrices.
3. **Undetermined Rate & Agreement Against Physician Gold-Standard Diagnoses**:
   - Across all **12,530** PHMRC verbal autopsies, SmartVA Tariff 2.0 assigned a specific primary cause of death for **11,627 records (`92.79%`)** and output `Undetermined` (tariff score below cause-specific cutoff or tied top rank) for **903 records (`7.21%`)**.
   - On the **11,627 determined records**, Top-1 agreement with the hospital physician gold-standard cause of death is **6,330 / 11,627 (`54.44%`)**, and Top-3 agreement (including secondary/tertiary likelihood predictions) is **8,078 / 11,627 (`69.48%`)**.
4. **Concordance with Published Tariff 2.0 Benchmarks (Serina et al., *BMC Medicine* 2015)**:
   - **Adult (`N = 7,841`)**: Mean Chance-Corrected Concordance ($CCC$) = **`49.89%`** on determined VAs (`46.78%` on all VAs), Mean Specificity = **`98.43%`**, CSMF Accuracy = **`0.8443`** (published Tariff 2.0 benchmark with HCE + free text: ~`48–50.5%` CCC, ~`0.77–0.83` CSMF accuracy).
   - **Child (`N = 2,064`)**: Mean $CCC$ = **`56.14%`** on determined VAs (`53.69%` on all VAs), Mean Specificity = **`97.42%`**, CSMF Accuracy = **`0.7937`** (published Tariff 2.0 benchmark: ~`52.5–55%` CCC, ~`0.78–0.80` CSMF accuracy).
   - **Neonate (`N = 2,625`)**: Mean $CCC$ = **`50.74%`** on determined VAs (`43.75%` on all VAs), Top-1 Agreement on determined VAs = **`70.19%`**, Mean Specificity = **`94.00%`**, CSMF Accuracy = **`0.8470`** (published Tariff 2.0 benchmark: ~`45–50%` CCC, ~`0.83–0.85` CSMF accuracy).

---

## 2. Module-Level Summary Table

| Metric | Adult (`34` Causes) | Child (`21` Causes) | Neonate (`6` Causes) | Combined PHMRC (`12,530` VAs) |
|---|---:|---:|---:|---:|
| **Total Gold-Standard Records ($N$)** | `7,841` | `2,064` | `2,625` | **`12,530`** |
| **Determined Predictions (`#` / `%`)** | `7,294` (`93.02%`) | `1,941` (`94.04%`) | `2,392` (`91.12%`) | **`11,627` (`92.79%`)** |
| **Undetermined / No Specific Cause (`#` / `%`)** | `547` (`6.98%`) | `123` (`5.96%`) | `233` (`8.88%`) | **`903` (`7.21%`)** |
| **Python `smartva.va_cli` vs. Rust `smartva-rs` Agreement** | `7,841/7,841` (**`100.00%`**) | `2,064/2,064` (**`100.00%`**) | `2,625/2,625` (**`100.00%`**) | **`12,530/12,530` (`100.00%`)** |
| **Top-1 Agreement vs. Physician Gold Standard (on Determined)** | `3,692/7,294` (**`50.62%`**) | `959/1,941` (**`49.41%`**) | `1,679/2,392` (**`70.19%`**) | **`6,330/11,627` (`54.44%`)** |
| **Top-1 Agreement vs. Physician Gold Standard (on All $N$)** | `3,692/7,841` (`47.09%`) | `959/2,064` (`46.46%`) | `1,679/2,625` (`63.96%`) | **`6,330/12,530` (`50.52%`)** |
| **Top-3 Agreement vs. Physician Gold Standard (on Determined)** | `4,904/7,294` (**`67.23%`**) | `1,253/1,941` (**`64.55%`**) | `1,921/2,392` (**`80.31%`**) | **`8,078/11,627` (`69.48%`)** |
| **Mean Cause Sensitivity (on Determined)** | `51.37%` | `58.23%` | `58.95%` | — |
| **Mean Cause Specificity (on Determined)** | `98.43%` | `97.42%` | `94.00%` | — |
| **Mean Positive Predictive Value (PPV)** | `50.21%` | `50.47%` | `57.58%` | — |
| **Mean Chance-Corrected Concordance ($CCC$, Determined)** | **`49.89%`** | **`56.14%`** | **`50.74%`** | — |
| **Mean Chance-Corrected Concordance ($CCC$, All $N$)** | `46.78%` | `53.69%` | `43.75%` | — |
| **CSMF Accuracy (vs. Gold-Standard CSMF)** | **`0.8443`** | **`0.7937`** | **`0.8470`** | — |
| **Runtime (`smartva.va_cli` Python)** | `89.17s` | `15.42s` | `13.62s` | **`118.21s`** |
| **Runtime (`smartva-rs` Pure Rust)** | **`4.48s`** (`19.9x` faster) | **`0.68s`** (`22.7x` faster) | **`0.77s`** (`17.7x` faster) | **`5.93s` (`19.9x` faster)** |

---

## 3. Adult Module Per-Cause Accuracy, Sensitivity, Specificity & CSMF (`N = 7,841`, 34 Causes)

| Cause of Death | Gold $N$ | Det. $N$ | Undet. $N$ (`%`) | Pred. $N$ | TP | Sens. Det (`%`) | Spec. Det (`%`) | PPV (`%`) | Top-3 Det (`%`) | $CCC_j$ (Det) | Gold CSMF | SmartVA CSMF |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| AIDS | `502` | `477` | `25` (`5.0%`) | `452` | `271` | `56.8%` | `97.3%` | `60.0%` | `82.4%` | `55.5%` | `0.0640` | `0.0620` |
| Bite of Venomous Animal | `66` | `65` | `1` (`1.5%`) | `77` | `61` | `93.8%` | `99.8%` | `79.2%` | `93.8%` | `93.7%` | `0.0084` | `0.0106` |
| Breast Cancer | `195` | `192` | `3` (`1.5%`) | `189` | `137` | `71.3%` | `99.3%` | `72.5%` | `84.4%` | `70.5%` | `0.0249` | `0.0259` |
| Cervical Cancer | `155` | `146` | `9` (`5.8%`) | `177` | `97` | `66.4%` | `98.9%` | `54.8%` | `77.4%` | `65.4%` | `0.0198` | `0.0243` |
| Chronic Kidney Disease | `416` | `382` | `34` (`8.2%`) | `312` | `160` | `41.9%` | `97.8%` | `51.3%` | `65.2%` | `40.1%` | `0.0531` | `0.0428` |
| Chronic Respiratory | `218` | `202` | `16` (`7.3%`) | `365` | `132` | `65.3%` | `96.7%` | `36.2%` | `89.6%` | `64.3%` | `0.0278` | `0.0500` |
| Cirrhosis | `313` | `300` | `13` (`4.2%`) | `336` | `183` | `61.0%` | `97.8%` | `54.5%` | `81.0%` | `59.8%` | `0.0399` | `0.0461` |
| Colorectal Cancer | `99` | `94` | `5` (`5.0%`) | `10` | `4` | `4.3%` | `99.9%` | `40.0%` | `6.4%` | `1.4%` | `0.0126` | `0.0014` |
| Diabetes | `414` | `394` | `20` (`4.8%`) | `412` | `171` | `43.4%` | `96.5%` | `41.5%` | `59.4%` | `41.7%` | `0.0528` | `0.0565` |
| Diarrhea/Dysentery | `228` | `187` | `41` (`18.0%`) | `56` | `31` | `16.6%` | `99.7%` | `55.4%` | `17.6%` | `14.1%` | `0.0291` | `0.0077` |
| Drowning | `106` | `102` | `4` (`3.8%`) | `97` | `89` | `87.2%` | `99.9%` | `91.8%` | `87.2%` | `86.9%` | `0.0135` | `0.0133` |
| Esophageal Cancer | `40` | `39` | `1` (`2.5%`) | `69` | `23` | `59.0%` | `99.4%` | `33.3%` | `61.5%` | `57.7%` | `0.0051` | `0.0095` |
| Falls | `173` | `165` | `8` (`4.6%`) | `288` | `106` | `64.2%` | `97.5%` | `36.8%` | `64.2%` | `63.2%` | `0.0221` | `0.0395` |
| Fires | `122` | `119` | `3` (`2.5%`) | `151` | `88` | `74.0%` | `99.1%` | `58.3%` | `74.0%` | `73.2%` | `0.0156` | `0.0207` |
| Homicide | `167` | `147` | `20` (`12.0%`) | `142` | `101` | `68.7%` | `99.4%` | `71.1%` | `68.7%` | `67.8%` | `0.0213` | `0.0195` |
| Ischemic Heart Disease | `621` | `583` | `38` (`6.1%`) | `584` | `317` | `54.4%` | `96.0%` | `54.3%` | `72.4%` | `53.0%` | `0.0792` | `0.0801` |
| Leukemia/Lymphomas | `156` | `145` | `11` (`7.0%`) | `178` | `76` | `52.4%` | `98.6%` | `42.7%` | `80.0%` | `51.0%` | `0.0199` | `0.0244` |
| Lung Cancer | `106` | `104` | `2` (`1.9%`) | `105` | `41` | `39.4%` | `99.1%` | `39.0%` | `78.8%` | `37.6%` | `0.0135` | `0.0144` |
| Malaria | `100` | `80` | `20` (`20.0%`) | `62` | `25` | `31.2%` | `99.5%` | `40.3%` | `33.8%` | `29.2%` | `0.0128` | `0.0085` |
| Maternal | `468` | `443` | `25` (`5.3%`) | `419` | `362` | `81.7%` | `99.2%` | `86.4%` | `93.5%` | `81.2%` | `0.0597` | `0.0574` |
| Other Cancers | `232` | `223` | `9` (`3.9%`) | `52` | `35` | `15.7%` | `99.7%` | `58.3%` | `63.7%` | `13.1%` | `0.0296` | `0.0071` |
| Other Cardiovascular Diseases | `195` | `180` | `15` (`7.7%`) | `19` | `2` | `1.1%` | `99.8%` | `10.5%` | `4.4%` | `-1.9%` | `0.0249` | `0.0026` |
| Other Infectious Diseases | `263` | `240` | `23` (`8.8%`) | `199` | `48` | `20.0%` | `97.9%` | `24.1%` | `43.3%` | `17.6%` | `0.0335` | `0.0273` |
| Other Injuries | `103` | `92` | `11` (`10.7%`) | `145` | `66` | `71.7%` | `98.9%` | `45.5%` | `71.7%` | `70.9%` | `0.0131` | `0.0199` |
| Other Non-communicable Diseases | `415` | `353` | `62` (`14.9%`) | `495` | `117` | `33.1%` | `94.7%` | `24.0%` | `54.7%` | `31.1%` | `0.0529` | `0.0679` |
| Pneumonia | `540` | `490` | `50` (`9.3%`) | `202` | `106` | `21.6%` | `98.6%` | `52.5%` | `40.8%` | `19.3%` | `0.0689` | `0.0277` |
| Poisonings | `86` | `73` | `13` (`15.1%`) | `69` | `44` | `60.3%` | `99.7%` | `63.8%` | `60.3%` | `59.1%` | `0.0110` | `0.0095` |
| Prostate Cancer | `48` | `47` | `1` (`2.1%`) | `103` | `29` | `61.7%` | `99.0%` | `28.2%` | `78.7%` | `60.5%` | `0.0061` | `0.0141` |
| Road Traffic | `202` | `199` | `3` (`1.5%`) | `254` | `166` | `83.4%` | `98.8%` | `65.3%` | `83.4%` | `82.9%` | `0.0258` | `0.0348` |
| Stomach Cancer | `62` | `61` | `1` (`1.6%`) | `97` | `24` | `39.3%` | `99.0%` | `24.7%` | `78.7%` | `37.5%` | `0.0079` | `0.0133` |
| Stroke | `630` | `594` | `36` (`5.7%`) | `829` | `404` | `68.0%` | `93.7%` | `48.7%` | `84.7%` | `67.0%` | `0.0803` | `0.1137` |
| Suicide | `124` | `116` | `8` (`6.5%`) | `59` | `38` | `32.8%` | `99.7%` | `64.4%` | `32.8%` | `30.7%` | `0.0158` | `0.0081` |
| TB | `276` | `260` | `16` (`5.8%`) | `290` | `138` | `53.1%` | `97.8%` | `47.6%` | `81.2%` | `51.7%` | `0.0352` | `0.0398` |

## 4. Child Module Per-Cause Accuracy, Sensitivity, Specificity & CSMF (`N = 2,064`, 21 Causes)

| Cause of Death | Gold $N$ | Det. $N$ | Undet. $N$ (`%`) | Pred. $N$ | TP | Sens. Det (`%`) | Spec. Det (`%`) | PPV (`%`) | Top-3 Det (`%`) | $CCC_j$ (Det) | Gold CSMF | SmartVA CSMF |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| AIDS | `20` | `19` | `1` (`5.0%`) | `113` | `14` | `73.7%` | `94.8%` | `12.4%` | `79.0%` | `72.4%` | `0.0097` | `0.0582` |
| Bite of Venomous Animal | `54` | `54` | `0` (`0.0%`) | `56` | `50` | `92.6%` | `99.7%` | `89.3%` | `92.6%` | `92.2%` | `0.0262` | `0.0289` |
| Childhood Cancer | `28` | `27` | `1` (`3.6%`) | `56` | `14` | `51.9%` | `97.8%` | `25.0%` | `85.2%` | `49.4%` | `0.0136` | `0.0289` |
| Childhood Cardiovascular Diseases | `76` | `65` | `11` (`14.5%`) | `23` | `8` | `12.3%` | `99.2%` | `34.8%` | `13.8%` | `7.9%` | `0.0368` | `0.0118` |
| Congenital malformation | `0` | `0` | `0` (`0.0%`) | `1` | `0` | `0.0%` | `100.0%` | `0.0%` | `0.0%` | `0.0%` | `0.0000` | `0.0005` |
| Diarrhea/Dysentery | `256` | `245` | `11` (`4.3%`) | `236` | `141` | `57.5%` | `94.4%` | `59.8%` | `74.3%` | `55.4%` | `0.1240` | `0.1216` |
| Digestive Diseases | `48` | `45` | `3` (`6.2%`) | `61` | `18` | `40.0%` | `97.7%` | `29.5%` | `53.3%` | `37.0%` | `0.0233` | `0.0314` |
| Drowning | `83` | `82` | `1` (`1.2%`) | `79` | `76` | `92.7%` | `99.8%` | `96.2%` | `92.7%` | `92.3%` | `0.0402` | `0.0407` |
| Encephalitis | `41` | `41` | `0` (`0.0%`) | `65` | `21` | `51.2%` | `97.7%` | `32.3%` | `75.6%` | `48.8%` | `0.0199` | `0.0335` |
| Falls | `49` | `46` | `3` (`6.1%`) | `68` | `31` | `67.4%` | `98.0%` | `45.6%` | `67.4%` | `65.8%` | `0.0237` | `0.0350` |
| Fires | `68` | `67` | `1` (`1.5%`) | `55` | `49` | `73.1%` | `99.7%` | `89.1%` | `73.1%` | `71.8%` | `0.0329` | `0.0283` |
| Hemorrhagic fever | `51` | `51` | `0` (`0.0%`) | `69` | `33` | `64.7%` | `98.1%` | `47.8%` | `76.5%` | `62.9%` | `0.0247` | `0.0355` |
| Homicide | `52` | `52` | `0` (`0.0%`) | `51` | `43` | `82.7%` | `99.6%` | `84.3%` | `82.7%` | `81.8%` | `0.0252` | `0.0263` |
| Malaria | `116` | `94` | `22` (`19.0%`) | `98` | `37` | `39.4%` | `96.7%` | `37.8%` | `40.4%` | `36.3%` | `0.0562` | `0.0505` |
| Measles | `23` | `23` | `0` (`0.0%`) | `84` | `19` | `82.6%` | `96.6%` | `22.6%` | `91.3%` | `81.7%` | `0.0111` | `0.0433` |
| Meningitis | `58` | `54` | `4` (`6.9%`) | `132` | `22` | `40.7%` | `94.2%` | `16.7%` | `59.3%` | `37.8%` | `0.0281` | `0.0680` |
| Other Defined Causes of Child Deaths | `194` | `177` | `17` (`8.8%`) | `99` | `53` | `29.9%` | `97.4%` | `53.5%` | `30.5%` | `26.4%` | `0.0940` | `0.0510` |
| Other Infectious Diseases | `67` | `63` | `4` (`6.0%`) | `64` | `20` | `31.8%` | `97.7%` | `31.2%` | `65.1%` | `28.3%` | `0.0325` | `0.0330` |
| Pneumonia | `532` | `506` | `26` (`4.9%`) | `260` | `178` | `35.2%` | `94.3%` | `68.5%` | `68.6%` | `31.9%` | `0.2578` | `0.1340` |
| Poisonings | `18` | `16` | `2` (`11.1%`) | `19` | `13` | `81.2%` | `99.7%` | `68.4%` | `81.2%` | `80.3%` | `0.0087` | `0.0098` |
| Preterm Delivery | `0` | `0` | `0` (`0.0%`) | `1` | `0` | `0.0%` | `100.0%` | `0.0%` | `0.0%` | `0.0%` | `0.0000` | `0.0005` |
| Road Traffic | `92` | `90` | `2` (`2.2%`) | `91` | `86` | `95.6%` | `99.7%` | `94.5%` | `95.6%` | `95.3%` | `0.0446` | `0.0469` |
| Sepsis | `138` | `124` | `14` (`10.1%`) | `160` | `33` | `26.6%` | `93.0%` | `20.6%` | `39.5%` | `22.9%` | `0.0669` | `0.0824` |

## 5. Neonate Module Per-Cause Accuracy, Sensitivity, Specificity & CSMF (`N = 2,625`, 6 Causes)

| Cause of Death | Gold $N$ | Det. $N$ | Undet. $N$ (`%`) | Pred. $N$ | TP | Sens. Det (`%`) | Spec. Det (`%`) | PPV (`%`) | Top-3 Det (`%`) | $CCC_j$ (Det) | Gold CSMF | SmartVA CSMF |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Birth asphyxia | `462` | `364` | `98` (`21.2%`) | `298` | `168` | `46.1%` | `93.6%` | `56.4%` | `50.8%` | `35.4%` | `0.1760` | `0.1246` |
| Congenital malformation | `249` | `214` | `35` (`14.1%`) | `139` | `92` | `43.0%` | `97.8%` | `66.2%` | `52.3%` | `31.6%` | `0.0949` | `0.0581` |
| Neonatal Meningitis/Sepsis | `166` | `148` | `18` (`10.8%`) | `263` | `104` | `70.3%` | `92.9%` | `39.5%` | `83.1%` | `64.3%` | `0.0632` | `0.1099` |
| Neonatal Pneumonia | `83` | `73` | `10` (`12.1%`) | `203` | `30` | `41.1%` | `92.5%` | `14.8%` | `67.1%` | `29.3%` | `0.0316` | `0.0849` |
| Pneumonia | `0` | `0` | `0` (`0.0%`) | `2` | `0` | `0.0%` | `99.9%` | `0.0%` | `0.0%` | `0.0%` | `0.0000` | `0.0008` |
| Preterm Delivery | `660` | `616` | `44` (`6.7%`) | `458` | `361` | `58.6%` | `94.5%` | `78.8%` | `85.7%` | `50.3%` | `0.2514` | `0.1915` |
| Stillbirth | `1005` | `977` | `28` (`2.8%`) | `1029` | `924` | `94.6%` | `92.6%` | `89.8%` | `94.6%` | `93.5%` | `0.3829` | `0.4302` |

