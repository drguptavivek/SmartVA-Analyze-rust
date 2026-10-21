//! Reverse-mapping preprocessor (`phmrc-source-prep`) that converts the GHDx PHMRC Gold Standard
//! Verbal Autopsy validation datasets (`IHME_PHMRC_VA_DATA_{ADULT,CHILD,NEONATE}_Y2013M09D11_0.csv`)
//! into standard ODK PHMRC Full Questionnaire CSVs that both upstream Python `SmartVA-Analyze`
//! (`smartva.va_cli`) and `smartva-rs` ingest natively.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::OnceLock;

use crate::data::{
    adult_pre_symptom_data, child_pre_symptom_data, common_data, neonate_pre_symptom_data,
    RecodeTarget,
};
use crate::SmartVaError;

/// Gold-standard validation metadata columns present in the GHDx PHMRC CSVs that are not part
/// of the raw ODK questionnaire.
pub static PHMRC_GOLD_STANDARD_META_COLS: &[&str] = &[
    "site",
    "module",
    "gs_code34",
    "gs_text34",
    "va34",
    "gs_code46",
    "gs_text46",
    "va46",
    "gs_code55",
    "gs_text55",
    "va55",
    "gs_comorbid1",
    "gs_comorbid2",
    "gs_level",
    "newid",
];

/// Direct pass-through columns present in the PHMRC validation CSVs that `SymptomPrep` reads
/// directly without a rename entry in `PreSymptomPrep::VAR_CONVERSION_MAP`.
pub static DIRECT_PASSTHROUGH_COLS: &[&str] = &[
    "a2_39_2",
    "a2_63_2",
    "c1_09",
    "c1_26",
    "c2_13a",
    "c4_31_2",
];

/// Unstemmed English trigger words that map 1-to-1 through `CommonPrep` (`WORD_SUBS`) and
/// `PreSymptomPrep` (`stem_porter2`) to each stem in `WORDS_TO_VARS`.
pub static STEM_TRIGGERS: &[(&str, &str)] = &[
    ("abdomen", "abdomen"),
    ("accid", "accid"),
    ("alcohol", "alcohol"),
    ("ami", "ami"),
    ("amput", "amput"),
    ("anemia", "anemia"),
    ("antibiot", "antibiot"),
    ("appetit", "appetit"),
    ("arrest", "arrest"),
    ("arteri", "arteri"),
    ("asphyxia", "asphyxia"),
    ("asthma", "asthma"),
    ("attack", "attack"),
    ("babi", "babi"),
    ("bacteria", "bacteria"),
    ("biopsi", "biopsi"),
    ("birth", "birth"),
    ("bite", "bite"),
    ("black", "black"),
    ("blood", "blood"),
    ("bluish", "bluish"),
    ("bone", "bone"),
    ("born", "born"),
    ("bowel", "bowel"),
    ("brain", "brain"),
    ("breast", "breast"),
    ("breath", "breath"),
    ("breech", "breech"),
    ("broken", "broken"),
    ("bwt", "bwt"),
    ("cancer", "cancer"),
    ("cardio", "cardio"),
    ("cardiomegali", "cardiomegali"),
    ("cathet", "cathet"),
    ("cerebr", "cerebr"),
    ("cervix", "cervix"),
    ("cesarean", "cesarean"),
    ("chemotherapi", "chemotherapi"),
    ("chest", "chest"),
    ("chronic", "chronic"),
    ("cirrhosi", "cirrhosi"),
    ("clot", "clot"),
    ("cold", "cold"),
    ("collaps", "collapse"),
    ("colon", "colon"),
    ("color", "color"),
    ("coma", "coma"),
    ("congest", "congest"),
    ("conscious", "conscious"),
    ("convuls", "convulse"),
    ("copd", "copd"),
    ("cord", "cord"),
    ("cough", "cough"),
    ("cri", "cri"),
    ("ctscan", "ctscan"),
    ("cut", "cut"),
    ("dehydr", "dehydr"),
    ("deliv", "deliv"),
    ("dengu", "dengu"),
    ("depress", "depress"),
    ("diabet", "diabet"),
    ("dialysi", "dialysi"),
    ("diarrhea", "diarrhea"),
    ("digest", "digest"),
    ("distress", "distress"),
    ("dizzi", "dizzi"),
    ("drink", "drink"),
    ("drown", "drown"),
    ("dyspnea", "dyspnea"),
    ("ecg", "ecg"),
    ("edema", "edema"),
    ("electr", "electr"),
    ("encephalopathi", "encephalopathi"),
    ("enlarg", "enlarg"),
    ("epilepsi", "epilepsi"),
    ("esophag", "esophag"),
    ("failur", "failur"),
    ("faint", "faint"),
    ("fall", "fall"),
    ("fetal", "fetal"),
    ("fever", "fever"),
    ("finger", "finger"),
    ("fire", "fire"),
    ("fluid", "fluid"),
    ("foot", "foot"),
    ("fractur", "fractur"),
    ("glucos", "glucose"),
    ("goiter", "goiter"),
    ("hand", "hand"),
    ("head", "head"),
    ("headach", "headach"),
    ("heart", "heart"),
    ("heartbeat", "heartbeat"),
    ("hematoma", "hematoma"),
    ("hemorrhag", "hemorrhag"),
    ("hepat", "hepat"),
    ("hernia", "hernia"),
    ("hiv", "hiv"),
    ("hypertens", "hypertense"),
    ("hypovolem", "hypovolem"),
    ("icu", "icu"),
    ("incub", "incub"),
    ("induc", "induc"),
    ("infarct", "infarct"),
    ("infect", "infect"),
    ("inflam", "inflam"),
    ("inhal", "inhal"),
    ("injuri", "injuri"),
    ("intestin", "intestin"),
    ("intracerebr", "intracerebr"),
    ("intub", "intub"),
    ("ischemia", "ischemia"),
    ("jaundic", "jaundic"),
    ("kerosen", "kerosen"),
    ("kidney", "kidney"),
    ("kill", "kill"),
    ("knee", "knee"),
    ("labor", "labor"),
    ("leukemia", "leukemia"),
    ("live", "live"),
    ("liver", "liver"),
    ("lump", "lump"),
    ("lung", "lung"),
    ("lymphoma", "lymphoma"),
    ("malaria", "malaria"),
    ("malnutrit", "malnutrit"),
    ("mass", "mass"),
    ("mellitus", "mellitus"),
    ("mental", "mental"),
    ("motorcycl", "motorcycl"),
    ("movement", "movement"),
    ("neck", "neck"),
    ("neonatorum", "neonatorum"),
    ("nephropathi", "nephropathi"),
    ("nerv", "nerv"),
    ("numb", "numb"),
    ("obstruct", "obstruct"),
    ("organ", "organ"),
    ("oxygen", "oxygen"),
    ("pain", "pain"),
    ("paralyz", "paralyz"),
    ("phlegm", "phlegm"),
    ("pneumonia", "pneumonia"),
    ("pneumothorax", "pneumothorax"),
    ("poison", "poison"),
    ("polic", "polic"),
    ("pox", "pox"),
    ("pregnanc", "pregnanc"),
    ("prenat", "prenat"),
    ("preterm", "preterm"),
    ("prostat", "prostat"),
    ("puffi", "puffi"),
    ("pulmonari", "pulmonari"),
    ("pus", "pus"),
    ("rash", "rash"),
    ("renal", "renal"),
    ("respiratori", "respiratori"),
    ("road", "road"),
    ("ruptur", "ruptur"),
    ("scan", "scan"),
    ("seizur", "seizur"),
    ("sepsi", "sepsi"),
    ("sever", "sever"),
    ("shock", "shock"),
    ("shot", "shot"),
    ("skin", "skin"),
    ("smoke", "smoke"),
    ("snake", "snake"),
    ("stillbirth", "stillbirth"),
    ("stomach", "stomach"),
    ("stomachach", "stomachach"),
    ("stone", "stone"),
    ("stool", "stool"),
    ("stress", "stress"),
    ("stroke", "stroke"),
    ("sugar", "sugar"),
    ("suicid", "suicid"),
    ("surgeri", "surgeri"),
    ("suspect", "suspect"),
    ("swell", "swell"),
    ("tetanus", "tetanus"),
    ("throat", "throat"),
    ("tongu", "tongu"),
    ("transfus", "transfus"),
    ("tree", "tree"),
    ("tumor", "tumor"),
    ("twin", "twin"),
    ("ulcer", "ulcer"),
    ("ultrasound", "ultrasound"),
    ("unconsci", "unconsci"),
    ("urin", "urin"),
    ("uterus", "uterus"),
    ("vascular", "vascular"),
    ("vehicular", "vehicular"),
    ("ventil", "ventil"),
    ("virus", "virus"),
    ("vomit", "vomit"),
    ("water", "water"),
    ("weight", "weight"),
    ("womb", "womb"),
    ("wound", "wound"),
    ("xray", "xray"),
    ("yellow", "yellow"),
];

/// Value label to integer code mappings extracted from `IHME_PHMRC_VA_DATA_CODEBOOK_Y2013M09D11_0.xlsx`.
pub static PHMRC_CODEBOOK_LABELS: &[(&str, &[(&str, &str)])] = &[
    ("gs_level", &[("GS Level 1", "1"), ("GS Level 2", "2"), ("GS Level 2B", "3"), ("Unknown", "9")]),
    ("g1_01d", &[("Don't Know", "99")]),
    ("g1_01m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g1_01y", &[("Don't Know", "9999")]),
    ("g1_05", &[("Male", "1"), ("Female", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("g1_06d", &[("Don't Know", "99")]),
    ("g1_06m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g1_06y", &[("Don't Know", "9999")]),
    ("g1_07a", &[("Don't Know", "999")]),
    ("g1_07b", &[("Don't Know", "99")]),
    ("g1_07c", &[("Don't Know", "99")]),
    ("g1_08", &[("Never Married", "1"), ("Married", "2"), ("Separated", "3"), ("Divorced", "4"), ("Widowed", "5"), ("No Data", "8"), ("Unknown", "9")]),
    ("g1_09", &[("No Schooling", "1"), ("Primary School", "2"), ("High School", "3"), ("College or Higher", "4"), ("Unknown", "9")]),
    ("g1_10", &[("Don't Know", "99")]),
    ("g2_03ad", &[("Don't Know", "99")]),
    ("g2_03am", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g2_03ay", &[("Don't Know", "9999")]),
    ("g2_03bd", &[("Don't Know", "99")]),
    ("g2_03bm", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g2_03by", &[("Don't Know", "9999")]),
    ("g2_03cd", &[("Don't Know", "99")]),
    ("g2_03cm", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g2_03cy", &[("Don't Know", "9999")]),
    ("g2_03dd", &[("Don't Know", "99")]),
    ("g2_03dm", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g2_03dy", &[("Don't Know", "9999")]),
    ("g2_03ed", &[("Don't Know", "99")]),
    ("g2_03em", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g2_03ey", &[("Don't Know", "9999")]),
    ("g2_03fd", &[("Don't Know", "99")]),
    ("g2_03fm", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g2_03fy", &[("Don't Know", "9999")]),
    ("g3_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("g4_02", &[("Male", "1"), ("Female", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("g4_03a", &[("Mother", "1"), ("Father", "2"), ("Grandmother", "3"), ("Grandfather", "4"), ("Aunt", "5"), ("Uncle", "6"), ("Husband", "7"), ("Wife", "8"), ("Brother", "9"), ("Sister", "10"), ("Birth Attendant", "11"), ("Other Male", "12"), ("Other Female", "13"), ("Unknown", "99")]),
    ("g4_05", &[("No Schooling", "1"), ("Primary School", "2"), ("High School", "3"), ("College or Higher", "4"), ("Unknown", "9")]),
    ("g4_08", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("g5_01d", &[("Don't Know", "99")]),
    ("g5_01m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g5_01y", &[("Don't Know", "9999")]),
    ("g5_02", &[("Male", "1"), ("Female", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("g5_03d", &[("Don't Know", "99")]),
    ("g5_03m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("g5_03y", &[("Don't Know", "9999")]),
    ("g5_04a", &[("Don't Know", "999")]),
    ("g5_04b", &[("Don't Know", "99")]),
    ("g5_04c", &[("Don't Know", "99")]),
    ("g5_05", &[("Never Married", "1"), ("Married", "2"), ("Separated", "3"), ("Divorced", "4"), ("Widowed", "5"), ("No Data", "8"), ("Unknown", "9")]),
    ("g5_06a", &[("No Schooling", "1"), ("Primary School", "2"), ("High School", "3"), ("College or Higher", "4"), ("Unknown", "9")]),
    ("g5_06b", &[("Don't Know", "99")]),
    ("g5_07", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_8", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_9", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a1_01_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_01a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_02", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_03a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_04", &[("Mild", "1"), ("Moderate", "2"), ("Severe", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_05", &[("Continuous", "1"), ("On and Off", "2"), ("Only at Night", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_06", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_07", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_08a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_09_1a", &[("Face", "1"), ("Trunk", "2"), ("Extremities", "3"), ("Everywhere", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_09_2a", &[("Face", "1"), ("Trunk", "2"), ("Extremities", "3"), ("Everywhere", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_15a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_16", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_17", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_18", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_19", &[("Slight", "1"), ("Moderate", "2"), ("Large", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_20", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_21", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_22a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_23", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_24a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_25", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_26a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_27", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_28a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_29", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_30", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_31", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_32", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_33a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_34", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_35", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_36", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_37a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_38", &[("Continuous", "1"), ("On and Off", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_39_1", &[("Lying", "1"), ("Sitting", "2"), ("Walking/Exertion", "3"), ("Didn't matter", "4"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_39_2", &[("Lying", "1"), ("Sitting", "2"), ("Walking/Exertion", "3"), ("Didn't matter", "4"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_40", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_41a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_42", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_43", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_44", &[("<30 minutes", "1"), ("0.5-24 hours", "2"), (">24 hr", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_45", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_46a", &[("Upper/middle chest", "1"), ("Lower chest", "2"), ("Left Arm", "3"), ("Other", "4"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_47", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_48a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_49", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_50", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_51", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_52", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_53", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_54a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_55", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_56", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_57", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_58a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_59", &[("Solids", "1"), ("Liquids", "2"), ("Both", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_60", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_61", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_62a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_63_1", &[("Upper belly", "1"), ("Lower belly", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_63_2", &[("Upper belly", "1"), ("Lower belly", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_64", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_65a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_66", &[("Rapidly/Fast", "1"), ("Slow(ly)", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_67", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_68a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_69", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_70a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_71", &[("Rapidly/Fast", "1"), ("Slow(ly)", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_72", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_73a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_74", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_75", &[("Suddenly", "1"), ("Slowly", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_76a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_77", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_78", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_79a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_80", &[("Suddenly", "1"), ("Slowly", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_81", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_82", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_83a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_84", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_85", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_86a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_8", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_9", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a2_87_10a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_02", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_03", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_04", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_05", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_06", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_07", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_08a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_09", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_11a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_15", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_16a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_17", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_18", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_19", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a3_20", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_5a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_02_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_05", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a4_06", &[("Low", "1"), ("Moderate", "2"), ("High", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_8", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_01_9a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_02", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_03", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a5_04a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_8", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_9", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_12a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_02_15", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_04", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_05", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_06_1d", &[("Don't Know", "99")]),
    ("a6_06_1m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("a6_06_1y", &[("Don't Know", "9999")]),
    ("a6_06_2d", &[("Don't Know", "99")]),
    ("a6_06_2m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("a6_06_2y", &[("Don't Know", "9999")]),
    ("a6_07d", &[("Don't Know", "99")]),
    ("a6_07m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("a6_07y", &[("Don't Know", "9999")]),
    ("a6_09", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("a6_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_01", &[("Singleton", "1"), ("Multiple", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_02", &[("First", "1"), ("Second", "2"), ("Third or More", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_03", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_04", &[("During", "1"), ("After", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_05a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_06a", &[("Hospital", "1"), ("Other Health Facility", "2"), ("On Route to Health Facility", "3"), ("Home", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_07", &[("Very small", "1"), ("smaller than usual", "2"), ("About average", "3"), ("larger than usual", "4"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_08a", &[("Grams", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_09", &[("Male", "1"), ("Female", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_10", &[("Specified", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_10d", &[("Don't Know", "99")]),
    ("c1_10m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("c1_10y", &[("Don't Know", "9999")]),
    ("c1_11", &[("Alive", "1"), ("Dead", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_15", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_16", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_17", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_18", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_19_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_19_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_19_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_19_4a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_19_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_19_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_20a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_21a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_22a", &[("Hospital", "1"), ("Other Health Facility", "2"), ("On Route to Health Facility", "3"), ("Home", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_24", &[("Specified", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_24d", &[("Don't Know", "99")]),
    ("c1_24m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("c1_24y", &[("Don't Know", "9999")]),
    ("c1_25a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c1_26", &[("Stillbirth", "0"), ("Less than 28 days", "1"), ("28 days to 11 years", "2")]),
    ("c2_01_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_8", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_9", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_01_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_02a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_03", &[("Early", "1"), ("On Time", "2"), ("Late", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_04", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_05a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_06", &[("Before", "1"), ("During", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_07", &[("< 1 day", "1"), ("One day or more", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_08a", &[("Green or brown", "1"), ("Clear (normal)", "2"), ("Other", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_09", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_10a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_12", &[("One", "1"), ("Two", "2"), ("Three", "3"), ("Four", "4"), ("5 or More", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_13a", &[("Hospital", "1"), ("Other Health Facility", "2"), ("On Route to Health Facility", "3"), ("Home", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_15a", &[("Doctor", "1"), ("Nurse/midwife", "2"), ("relative", "3"), ("Self/mother", "4"), ("Traditional birth attendant", "5"), ("Other", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_17", &[("Vaginal with Forceps", "1"), ("Vaginal without Forceps", "2"), ("Vaginal Don't Know", "3"), ("C-Section", "4"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c2_18", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_02", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_03_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_03_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_03_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_03_4a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_03_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_03_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_04", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_05", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_06", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_07", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_08", &[("Within 5 minutes", "1"), ("Within 6-30 min.", "2"), ("More than 30 minutes", "3"), ("Never", "4"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_09", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_10", &[("< 1 day", "1"), ("One day or more", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_14a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_15", &[("< 1 day", "1"), ("One day or more", "2"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_16", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_17", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_18a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_19a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_20", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_21a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_22a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_23", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_24", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_25", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_26", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_27a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_28a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_29", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_30a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_31a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_32", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_33", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_34", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_35", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_36", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_37", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_38", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_39", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_40", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_41", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_42", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_44", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_45a", &[("Specified", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_46", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_47", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_48", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c3_49", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_02a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_03", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_04", &[("Mild", "1"), ("Moderate", "2"), ("Severe", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_05", &[("Continuous", "1"), ("On and Off", "2"), ("Only at Night", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_06", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_07a", &[("Specified", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_08a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_09", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_10a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_13a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_15", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_16", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_17a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_18", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_19a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_20", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_22", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_23", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_24", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_25", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_26", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_27", &[("<6 hours", "1"), ("6-23 hours", "2"), ("24 hours or more", "3"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_28", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_29", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_30", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_31_1", &[("Face", "1"), ("Trunk", "2"), ("Extremities", "3"), ("Everywhere", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_31_2", &[("Face", "1"), ("Trunk", "2"), ("Extremities", "3"), ("Everywhere", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_32", &[("Face", "1"), ("Trunk", "2"), ("Extremities", "3"), ("Everywhere", "4"), ("Other", "5"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_33a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_34", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_35", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_36", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_37a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_38", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_39", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_40", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_41", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_42", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_43", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_44", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_46", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_8a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_9", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_47_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_48", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c4_49a", &[("Years", "1"), ("Months", "2"), ("Weeks", "3"), ("Days", "4"), ("Hours", "5"), ("Minutes", "6"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_01", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_1", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_2", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_3", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_4", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_5", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_6", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_7", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_8", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_9", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_11a", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_12", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_13", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_02_14", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_04", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_05", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_06_1d", &[("Don't Know", "99")]),
    ("c5_06_1m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("c5_06_1y", &[("Don't Know", "9999")]),
    ("c5_06_2d", &[("Don't Know", "99")]),
    ("c5_06_2m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("c5_06_2y", &[("Don't Know", "9999")]),
    ("c5_08d", &[("Don't Know", "99")]),
    ("c5_08m", &[("January", "1"), ("February", "2"), ("March", "3"), ("April", "4"), ("May", "5"), ("June", "6"), ("July", "7"), ("August", "8"), ("September", "9"), ("October", "10"), ("November", "11"), ("December", "12"), ("Don't Know", "99")]),
    ("c5_08y", &[("Don't Know", "9999")]),
    ("c5_10", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_11", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_17", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_18", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
    ("c5_19", &[("No", "0"), ("Yes", "1"), ("Refused to Answer", "8"), ("Don't Know", "9")]),
];


type CodebookLookup = HashMap<&'static str, HashMap<String, &'static str>>;

fn get_codebook_lookup() -> &'static CodebookLookup {
    static LOOKUP: OnceLock<CodebookLookup> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        let mut map = HashMap::with_capacity(PHMRC_CODEBOOK_LABELS.len());
        for &(var, entries) in PHMRC_CODEBOOK_LABELS {
            let mut inner = HashMap::with_capacity(entries.len() * 2);
            for &(lbl, code) in entries {
                inner.insert(lbl.trim().to_string(), code);
                inner.insert(lbl.trim().to_ascii_lowercase(), code);
            }
            map.insert(var, inner);
        }
        map
    })
}

/// Decode a single cell from a GHDx PHMRC Gold Standard CSV into its numeric/raw ODK representation.
pub fn decode_phmrc_cell(var_name: &str, raw_val: &str) -> String {
    let trimmed = raw_val.trim();
    if trimmed.is_empty() || trimmed == "NA" || trimmed == "." {
        return String::new();
    }
    if let Some(var_map) = get_codebook_lookup().get(var_name) {
        if let Some(&code) = var_map.get(trimmed) {
            return code.to_string();
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(&code) = var_map.get(&lower) {
            return code.to_string();
        }
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "yes" => "1".to_string(),
        "no" => "0".to_string(),
        "refused to answer" => "8".to_string(),
        "don't know" => "9".to_string(),
        _ => trimmed.to_string(),
    }
}

fn is_multiselect_intermediate(odk_col: &str) -> bool {
    odk_col.starts_with("adultrash")
        || odk_col.starts_with("adultparalysis")
        || odk_col.starts_with("adultinjury")
        || odk_col.starts_with("childabnorm")
        || odk_col.starts_with("complications")
        || odk_col.starts_with("childinjury")
        || odk_col.starts_with("provider")
}

fn is_short_form_word_col(odk_col: &str) -> bool {
    odk_col.starts_with("adult_7_") && odk_col != "adult_7_c"
        || odk_col.starts_with("child_6_") && odk_col != "child_6_c"
        || odk_col.starts_with("neonate_6_")
}

fn is_weight_or_free_text_special(odk_col: &str) -> bool {
    matches!(
        odk_col,
        "child_1_8"
            | "child_1_8a"
            | "child_1_8b"
            | "child_5_6e"
            | "child_5_6f"
            | "child_5_6g"
            | "child_5_7e"
            | "child_5_7f"
            | "child_5_7g"
            | "agedays"
            | "adult_5_1"
            | "adult_6_2"
            | "child_4_47"
    ) || common_data::FREE_TEXT_VARS.contains(&odk_col)
}

/// Convert a GHDx PHMRC Gold Standard CSV file (`IHME_PHMRC_VA_DATA_{ADULT,CHILD,NEONATE}_Y2013M09D11_0.csv`)
/// into a standard ODK PHMRC Full Questionnaire CSV file ready for `SmartVA-Analyze` (`smartva.va_cli`) or `smartva-rs`.
/// Returns the number of converted VA records.
pub fn convert_phmrc_gold_standard_csv(
    input_csv: &Path,
    output_csv: &Path,
) -> Result<usize, SmartVaError> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(input_csv)
        .map_err(|e| {
            SmartVaError::IoError(format!("Failed to open PHMRC CSV {}: {}", input_csv.display(), e))
        })?;

    let in_headers: Vec<String> = rdr
        .headers()
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?
        .iter()
        .map(|s| s.trim().to_string())
        .collect();
    let in_header_idx: HashMap<&str, usize> = in_headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.as_str(), i))
        .collect();

    // Build duration variable metadata across adult, child, neonate
    let all_dur_vars: HashSet<&str> = adult_pre_symptom_data::DURATION_VARS
        .iter()
        .chain(child_pre_symptom_data::DURATION_VARS.iter())
        .chain(neonate_pre_symptom_data::DURATION_VARS.iter())
        .chain(child_pre_symptom_data::DURATION_DAYS_VARS.iter())
        .copied()
        .collect();
    let dur_unit_targets: HashSet<String> = all_dur_vars
        .iter()
        .flat_map(|v| [format!("{}a", v), format!("{}b", v)])
        .collect();

    // Build inverse VAR_CONVERSION_MAP: phmrc_col -> odk_col
    let mut inv_var_map: Vec<(&'static str, &'static str)> = Vec::new();
    let mut seen_odk: HashSet<&'static str> = HashSet::new();
    for &(odk_col, phmrc_col) in adult_pre_symptom_data::VAR_CONVERSION_MAP
        .iter()
        .chain(child_pre_symptom_data::VAR_CONVERSION_MAP.iter())
        .chain(neonate_pre_symptom_data::VAR_CONVERSION_MAP.iter())
    {
        if odk_col == "sid"
            || is_multiselect_intermediate(odk_col)
            || is_short_form_word_col(odk_col)
            || is_weight_or_free_text_special(odk_col)
            || dur_unit_targets.contains(phmrc_col)
        {
            continue;
        }
        if seen_odk.insert(odk_col) {
            inv_var_map.push((odk_col, phmrc_col));
        }
    }

    // Map {dur_var}a -> odk_unit_col and {dur_var}b -> odk_len_col
    let mut dur_va_to_odk: HashMap<&'static str, &'static str> = HashMap::new();
    let mut dur_vb_to_odk: HashMap<&'static str, &'static str> = HashMap::new();
    for &(odk_col, phmrc_col) in adult_pre_symptom_data::VAR_CONVERSION_MAP
        .iter()
        .chain(child_pre_symptom_data::VAR_CONVERSION_MAP.iter())
        .chain(neonate_pre_symptom_data::VAR_CONVERSION_MAP.iter())
    {
        if dur_unit_targets.contains(phmrc_col) {
            if phmrc_col.ends_with('a') {
                dur_va_to_odk.entry(phmrc_col).or_insert(odk_col);
            } else if phmrc_col.ends_with('b') && odk_col != "agedays" {
                dur_vb_to_odk.entry(phmrc_col).or_insert(odk_col);
            }
        }
    }

    // Build recode_map lookup: {dur_var}b -> Vec<(unit_code, odk_col)>
    let mut recode_lookup: HashMap<&'static str, Vec<(i32, &'static str)>> = HashMap::new();
    for &((_read_h, write_h), entries) in adult_pre_symptom_data::RECODE_MAP
        .iter()
        .chain(child_pre_symptom_data::RECODE_MAP.iter())
        .chain(neonate_pre_symptom_data::RECODE_MAP.iter())
    {
        let list = recode_lookup.entry(write_h).or_default();
        for &(code, target) in entries {
            if let RecodeTarget::Column(col) = target {
                if !list.iter().any(|&(c, _)| c == code) {
                    list.push((code, col));
                }
            }
        }
    }

    let stem_trigger_map: HashMap<&'static str, &'static str> =
        STEM_TRIGGERS.iter().copied().collect();

    // Collect output headers in deterministic order
    let mut out_header_set: BTreeSet<String> = BTreeSet::new();
    for &(odk_col, phmrc_col) in &inv_var_map {
        if in_header_idx.contains_key(phmrc_col) {
            out_header_set.insert(odk_col.to_string());
        }
    }
    for &col in &[
        "sid",
        "gen_1_2a",
        "gen_3_1",
        "gen_5_4a",
        "gen_5_4b",
        "gen_5_4c",
        "gen_5_4d",
        "adult_2_9",
        "adult_2_87",
        "adult_4_2",
        "adult_5_1",
        "adult_5_2",
        "adult_6_2",
        "adult_7_9",
        "a_7_11",
        "adult_7_c",
        "child_1_8",
        "child_1_8a",
        "child_1_8b",
        "child_1_19",
        "child_2_1",
        "child_3_3",
        "child_4_47",
        "child_4_48",
        "child_5_2",
        "child_5_6e",
        "child_5_6f",
        "child_5_6g",
        "child_5_7e",
        "child_5_7f",
        "child_5_7g",
        "child_6_c",
        "agedays",
    ] {
        out_header_set.insert(col.to_string());
    }
    for &col in DIRECT_PASSTHROUGH_COLS {
        if in_header_idx.contains_key(col) {
            out_header_set.insert(col.to_string());
        }
    }
    for &dvar in &all_dur_vars {
        if in_header_idx.contains_key(dvar) {
            let va = format!("{}a", dvar);
            let vb = format!("{}b", dvar);
            if let Some(&odk_a) = dur_va_to_odk.get(va.as_str()) {
                out_header_set.insert(odk_a.to_string());
            }
            if let Some(&odk_b) = dur_vb_to_odk.get(vb.as_str()) {
                out_header_set.insert(odk_b.to_string());
            }
            if let Some(rec_cols) = recode_lookup.get(vb.as_str()) {
                for &(_, rcol) in rec_cols {
                    out_header_set.insert(rcol.to_string());
                }
            }
            out_header_set.insert(vb);
        }
    }
    if let Some(rec_cols) = recode_lookup.get("g4_03b") {
        for &(_, rcol) in rec_cols {
            out_header_set.insert(rcol.to_string());
        }
    }

    let mut out_headers: Vec<String> = vec!["sid".to_string()];
    for h in out_header_set {
        if h != "sid" {
            out_headers.push(h);
        }
    }

    let file = File::create(output_csv).map_err(|e| {
        SmartVaError::IoError(format!(
            "Failed to create output CSV {}: {}",
            output_csv.display(),
            e
        ))
    })?;
    let mut wtr = csv::WriterBuilder::new().from_writer(BufWriter::new(file));
    wtr.write_record(&out_headers)
        .map_err(|e| SmartVaError::CsvError(e.to_string()))?;

    let mut row_count = 0usize;
    for (row_idx, rec_res) in rdr.byte_records().enumerate() {
        let rec = rec_res.map_err(|e| SmartVaError::CsvError(e.to_string()))?;
        row_count += 1;

        let get_raw = |col: &str| -> String {
            in_header_idx
                .get(col)
                .and_then(|&i| rec.get(i))
                .map(|b| String::from_utf8_lossy(b).trim().to_string())
                .unwrap_or_default()
        };
        let get_dec = |col: &str| -> String { decode_phmrc_cell(col, &get_raw(col)) };

        let mut out_row: HashMap<String, String> = HashMap::with_capacity(out_headers.len());

        // 1. SID, site, consent, and module
        let newid = get_raw("newid");
        let sid = if !newid.is_empty() {
            newid
        } else {
            (row_idx + 1).to_string()
        };
        out_row.insert("sid".to_string(), sid);
        let site = get_raw("site");
        if !site.is_empty() {
            out_row.insert("gen_1_2a".to_string(), site);
        }
        let g3_01 = get_dec("g3_01");
        out_row.insert(
            "gen_3_1".to_string(),
            if g3_01.is_empty() {
                "1".to_string()
            } else {
                g3_01
            },
        );

        let mod_raw = get_raw("module").to_ascii_lowercase();
        let mod_code = match mod_raw.as_str() {
            "adult" => "3",
            "child" => "2",
            "neonate" => "1",
            _ => "3",
        };
        out_row.insert("gen_5_4d".to_string(), mod_code.to_string());

        // 2. Direct inverse VAR_CONVERSION_MAP
        for &(odk_col, phmrc_col) in &inv_var_map {
            if in_header_idx.contains_key(phmrc_col) {
                let v = get_dec(phmrc_col);
                if !v.is_empty() {
                    out_row.insert(odk_col.to_string(), v);
                }
            }
        }
        for &col in DIRECT_PASSTHROUGH_COLS {
            if in_header_idx.contains_key(col) {
                let v = get_dec(col);
                if !v.is_empty() {
                    out_row.insert(col.to_string(), v);
                }
            }
        }

        // g4_03b specify column
        let g4_03b_val = get_raw("g4_03b");
        if !g4_03b_val.is_empty() && g4_03b_val != "NA" && g4_03b_val != "." {
            match get_dec("g4_03a").as_str() {
                "11" => {
                    out_row.insert("gen_4_3a".to_string(), g4_03b_val);
                }
                "12" => {
                    out_row.insert("gen_4_3b".to_string(), g4_03b_val);
                }
                "13" => {
                    out_row.insert("gen_4_3c".to_string(), g4_03b_val);
                }
                _ => {}
            }
        }

        // 3. Multiselect & binary expansions
        // 3a. adult_2_9 (rash locations)
        let mut rash_codes = Vec::new();
        for rcol in ["a2_09_1a", "a2_09_2a", "a2_09_3a"] {
            let v = get_dec(rcol);
            if !v.is_empty() && v != "0" {
                rash_codes.push(v);
            }
        }
        if !rash_codes.is_empty() {
            out_row.insert("adult_2_9".to_string(), rash_codes.join(" "));
        }

        let build_multiselect = |pairs: &[(i32, &str)]| -> String {
            let mut codes = Vec::new();
            for &(code, col) in pairs {
                if get_dec(col) == "1" {
                    codes.push(code.to_string());
                }
            }
            codes.join(" ")
        };

        let a2_87 = build_multiselect(&[
            (1, "a2_87_1"),
            (2, "a2_87_2"),
            (3, "a2_87_3"),
            (4, "a2_87_4"),
            (5, "a2_87_5"),
            (6, "a2_87_6"),
            (7, "a2_87_7"),
            (8, "a2_87_8"),
            (9, "a2_87_9"),
            (11, "a2_87_10a"),
        ]);
        if !a2_87.is_empty() {
            out_row.insert("adult_2_87".to_string(), a2_87);
        }

        let a4_02 = build_multiselect(&[
            (1, "a4_02_1"),
            (2, "a4_02_2"),
            (3, "a4_02_3"),
            (4, "a4_02_4"),
            (11, "a4_02_5a"),
            (8, "a4_02_6"),
            (9, "a4_02_7"),
        ]);
        if !a4_02.is_empty() {
            out_row.insert("adult_4_2".to_string(), a4_02);
        }

        let a5_02_ms = build_multiselect(&[
            (1, "a5_01_1"),
            (2, "a5_01_2"),
            (3, "a5_01_3"),
            (4, "a5_01_4"),
            (5, "a5_01_5"),
            (6, "a5_01_6"),
            (7, "a5_01_7"),
            (11, "a5_01_9a"),
        ]);
        if !a5_02_ms.is_empty() {
            out_row.insert("adult_5_2".to_string(), a5_02_ms.clone());
        }
        if get_dec("a5_01_8") == "1" {
            out_row.insert("adult_5_1".to_string(), "0".to_string());
        } else if !a5_02_ms.is_empty() {
            out_row.insert("adult_5_1".to_string(), "1".to_string());
        }

        let a6_02 = build_multiselect(&[
            (1, "a6_02_1"),
            (2, "a6_02_2"),
            (3, "a6_02_3"),
            (4, "a6_02_4"),
            (5, "a6_02_5"),
            (6, "a6_02_6"),
            (7, "a6_02_8"),
            (8, "a6_02_9"),
            (9, "a6_02_10"),
            (10, "a6_02_11"),
            (11, "a6_02_12a"),
            (12, "a6_02_13"),
            (88, "a6_02_14"),
            (99, "a6_02_15"),
        ]);
        if !a6_02.is_empty() {
            out_row.insert("adult_6_2".to_string(), a6_02);
        }

        let c1_19 = build_multiselect(&[
            (1, "c1_19_1"),
            (2, "c1_19_2"),
            (3, "c1_19_3"),
            (11, "c1_19_4a"),
            (8, "c1_19_5"),
        ]);
        if !c1_19.is_empty() {
            out_row.insert("child_1_19".to_string(), c1_19);
        }

        let c3_03 = build_multiselect(&[
            (1, "c3_03_1"),
            (2, "c3_03_2"),
            (3, "c3_03_3"),
            (11, "c3_03_4a"),
            (8, "c3_03_5"),
        ]);
        if !c3_03.is_empty() {
            out_row.insert("child_3_3".to_string(), c3_03);
        }

        let c2_01 = build_multiselect(&[
            (1, "c2_01_1"),
            (2, "c2_01_2"),
            (3, "c2_01_3"),
            (4, "c2_01_4"),
            (5, "c2_01_5"),
            (6, "c2_01_6"),
            (7, "c2_01_7"),
            (8, "c2_01_8"),
            (9, "c2_01_9"),
            (10, "c2_01_10"),
            (88, "c2_01_11"),
            (99, "c2_01_12"),
        ]);
        if !c2_01.is_empty() {
            out_row.insert("child_2_1".to_string(), c2_01);
        }

        let c4_48 = build_multiselect(&[
            (1, "c4_47_1"),
            (2, "c4_47_2"),
            (3, "c4_47_3"),
            (4, "c4_47_4"),
            (5, "c4_47_5"),
            (6, "c4_47_6"),
            (7, "c4_47_7"),
            (11, "c4_47_8a"),
            (8, "c4_47_10"),
            (9, "c4_47_9"),
        ]);
        if !c4_48.is_empty() {
            out_row.insert("child_4_48".to_string(), c4_48.clone());
        }
        if get_dec("c4_47_11") == "1" {
            out_row.insert("child_4_47".to_string(), "0".to_string());
        } else if !c4_48.is_empty() {
            out_row.insert("child_4_47".to_string(), "1".to_string());
        }

        let c5_02 = build_multiselect(&[
            (1, "c5_02_1"),
            (2, "c5_02_2"),
            (3, "c5_02_3"),
            (4, "c5_02_4"),
            (5, "c5_02_5"),
            (6, "c5_02_6"),
            (7, "c5_02_7"),
            (8, "c5_02_8"),
            (9, "c5_02_9"),
            (10, "c5_02_10"),
            (11, "c5_02_11a"),
            (12, "c5_02_12"),
            (88, "c5_02_13"),
            (99, "c5_02_14"),
        ]);
        if !c5_02.is_empty() {
            out_row.insert("child_5_2".to_string(), c5_02);
        }

        // 4. Weights (grams)
        let c1_08b = get_dec("c1_08b");
        if !c1_08b.is_empty() {
            out_row.insert("child_1_8".to_string(), "1".to_string());
            out_row.insert("child_1_8a".to_string(), c1_08b);
        }
        let c5_07_1 = get_dec("c5_07_1");
        if !c5_07_1.is_empty() {
            out_row.insert("child_5_6e".to_string(), "1".to_string());
            out_row.insert("child_5_6f".to_string(), c5_07_1);
        }
        let c5_07_2 = get_dec("c5_07_2");
        if !c5_07_2.is_empty() {
            out_row.insert("child_5_7e".to_string(), "1".to_string());
            out_row.insert("child_5_7f".to_string(), c5_07_2);
        }

        if mod_code == "1" {
            if out_row.get("gen_5_4c").map(|s| s.as_str()) == Some("99") {
                out_row.insert("gen_5_4c".to_string(), "0".to_string());
            }
            let c1_25_raw = get_raw("c1_25");
            if out_row.get("gen_5_4c").map(|s| s.is_empty()).unwrap_or(true)
                && c1_25_raw.parse::<i32>().map(|v| v > 0).unwrap_or(false)
            {
                if out_row.get("gen_5_4a").map(|s| s.as_str()) == Some(c1_25_raw.as_str())
                    || out_row.get("gen_5_4b").map(|s| s.as_str()) == Some(c1_25_raw.as_str())
                {
                    out_row.insert("gen_5_4c".to_string(), c1_25_raw);
                    out_row.remove("gen_5_4a");
                    out_row.remove("gen_5_4b");
                }
            }
            let c1_09_raw = get_raw("c1_09").to_ascii_lowercase();
            if c1_09_raw == "female" {
                out_row.insert("c1_09".to_string(), "1.0".to_string());
            } else if c1_09_raw == "male" {
                out_row.insert("c1_09".to_string(), "0.0".to_string());
            } else if c1_09_raw == "don't know" {
                out_row.insert("c1_09".to_string(), "9.0".to_string());
            } else if c1_09_raw.is_empty() {
                out_row.remove("c1_09");
            }
            let c2_13a_raw = get_raw("c2_13a").to_ascii_lowercase();
            if c2_13a_raw == "home" {
                out_row.insert("c2_13a".to_string(), "1".to_string());
            } else if c2_13a_raw == "other" {
                out_row.insert("c2_13a".to_string(), "2".to_string());
            } else if !c2_13a_raw.is_empty() {
                out_row.insert("c2_13a".to_string(), "3".to_string());
            }
            let mut w1_f = get_raw("c5_07_1").parse::<f64>().unwrap_or(0.0);
            let mut w2_f = get_raw("c5_07_2").parse::<f64>().unwrap_or(0.0);
            if w1_f > 0.3 && w1_f < 10.0 {
                w1_f *= 1000.0;
            }
            if w2_f > 0.3 && w2_f < 10.0 {
                w2_f *= 1000.0;
            }
            let valid_w2 = w2_f > 300.0 && w2_f < 9000.0 && (w2_f - 999.0).abs() > 1e-6;
            let valid_w1 = w1_f > 300.0 && w1_f < 9000.0 && (w1_f - 999.0).abs() > 1e-6;
            let chosen_w = if valid_w2 {
                w2_f
            } else if valid_w1 {
                w1_f
            } else {
                0.0
            };
            if chosen_w > 0.0 {
                let dob_ok = out_row
                    .get("gen_5_1a")
                    .and_then(|s| s.parse::<i32>().ok())
                    .is_some()
                    && out_row
                        .get("gen_5_1b")
                        .and_then(|s| s.parse::<i32>().ok())
                        .is_some()
                    && out_row
                        .get("gen_5_1c")
                        .and_then(|s| s.parse::<i32>().ok())
                        .is_some();
                if !dob_ok {
                    out_row.insert("gen_5_1a".to_string(), "2008".to_string());
                    out_row.insert("gen_5_1b".to_string(), "1".to_string());
                    out_row.insert("gen_5_1c".to_string(), "10".to_string());
                }
                let y_i = out_row["gen_5_1a"].parse::<i32>().unwrap_or(2008);
                let m_i = out_row["gen_5_1b"].parse::<i32>().unwrap_or(1);
                let d_i = out_row["gen_5_1c"].parse::<i32>().unwrap_or(10).min(27);
                out_row.insert("gen_5_1c".to_string(), d_i.to_string());
                out_row.insert("child_5_6b".to_string(), y_i.to_string());
                out_row.insert("child_5_6c".to_string(), m_i.to_string());
                out_row.insert("child_5_6d".to_string(), (d_i + 1).to_string());
                out_row.insert("child_5_6e".to_string(), "1".to_string());
                out_row.insert(
                    "child_5_6f".to_string(),
                    (chosen_w.round() as i32).to_string(),
                );
                for k_clear in [
                    "child_5_7b",
                    "child_5_7c",
                    "child_5_7d",
                    "child_5_7e",
                    "child_5_7f",
                ] {
                    out_row.remove(k_clear);
                }
            } else {
                for k_clear in [
                    "child_5_6b",
                    "child_5_6c",
                    "child_5_6d",
                    "child_5_6e",
                    "child_5_6f",
                    "child_5_7b",
                    "child_5_7c",
                    "child_5_7d",
                    "child_5_7e",
                    "child_5_7f",
                ] {
                    out_row.remove(k_clear);
                }
            }
        }

        // 5. Duration variables
        for &dvar in &all_dur_vars {
            if !in_header_idx.contains_key(dvar) {
                continue;
            }
            let raw_d = get_dec(dvar);
            if raw_d.is_empty() {
                continue;
            }
            if dvar == "c1_25" {
                let agedays_val = if mod_code == "1"
                    && get_raw("c1_26").eq_ignore_ascii_case("stillbirth")
                    && raw_d == "2"
                {
                    "0".to_string()
                } else if mod_code == "1" && raw_d == "1" {
                    "3".to_string()
                } else {
                    raw_d
                };
                out_row.insert("agedays".to_string(), agedays_val);
                continue;
            }
            let Ok(mut days_val) = raw_d.parse::<f64>() else {
                continue;
            };
            if mod_code == "1"
                && (days_val - 1.0).abs() < 1e-9
                && matches!(
                    dvar,
                    "c1_20"
                        | "c1_21"
                        | "c3_14"
                        | "c3_18"
                        | "c3_19"
                        | "c3_21"
                        | "c3_22"
                        | "c3_27"
                        | "c3_28"
                        | "c3_30"
                        | "c3_31"
                )
            {
                days_val = 3.0;
            }
            let va = format!("{}a", dvar);
            let vb = format!("{}b", dvar);

            // In Child module, c4_02, c4_08, c4_10 are DURATION_DAYS_VARS (unit=2 for >=1 day, 1 for 0)
            if mod_code == "2" && matches!(dvar, "c4_02" | "c4_08" | "c4_10") {
                let iv = days_val.round() as i32;
                let u_str = if iv > 0 { "2" } else { "1" };
                if let Some(&odk_a) = dur_va_to_odk.get(va.as_str()) {
                    out_row.insert(odk_a.to_string(), u_str.to_string());
                }
                if let Some(&odk_b) = dur_vb_to_odk.get(vb.as_str()) {
                    out_row.insert(odk_b.to_string(), iv.to_string());
                }
                continue;
            }

            // Standard DURATION_VARS (1=Years/365, 2=Months/30, 3=Weeks/7, 4=Days/1, 5=Hours/(1/24), 6=Minutes/(1/1440))
            let (unit_code, len_int) = if (days_val - days_val.round()).abs() < 1e-6 {
                (4, days_val.round() as i32)
            } else if ((days_val * 24.0) - (days_val * 24.0).round()).abs() < 1e-3 {
                (5, (days_val * 24.0).round() as i32)
            } else {
                (6, (days_val * 1440.0).round() as i32)
            };

            if let Some(&odk_a) = dur_va_to_odk.get(va.as_str()) {
                out_row.insert(odk_a.to_string(), unit_code.to_string());
            }
            if let Some(&odk_b) = dur_vb_to_odk.get(vb.as_str()) {
                out_row.insert(odk_b.to_string(), len_int.to_string());
            }
            if let Some(rec_cols) = recode_lookup.get(vb.as_str()) {
                for &(c, rcol) in rec_cols {
                    if c == unit_code {
                        out_row.insert(rcol.to_string(), len_int.to_string());
                    }
                }
            }
            out_row.insert(vb, len_int.to_string());
        }

        // 6. Free-text stems (`word_<stem>`) -> trigger words in `adult_7_c` / `child_6_c`
        let mut trigger_words = Vec::new();
        for (&stem, &trigger) in &stem_trigger_map {
            let wcol = format!("word_{}", stem);
            if in_header_idx.contains_key(wcol.as_str()) {
                let v = get_dec(&wcol);
                if let Ok(fv) = v.parse::<f64>() {
                    if fv >= 1.0 {
                        trigger_words.push(trigger);
                    }
                }
            }
        }
        trigger_words.sort_unstable();
        if !trigger_words.is_empty() {
            let joined = trigger_words.join(" ");
            if mod_code == "3" {
                out_row.insert("adult_7_c".to_string(), joined);
            } else {
                out_row.insert("child_6_c".to_string(), joined);
            }
        }

        // Also populate `adult_7_9` (pneumonia) and `a_7_11` (suicide) for Adult RulesPrep
        if mod_code == "3" {
            let pneu = get_dec("word_pneumonia")
                .parse::<f64>()
                .map(|v| if v >= 1.0 { "1" } else { "0" })
                .unwrap_or("0");
            let suic = get_dec("word_suicid")
                .parse::<f64>()
                .map(|v| if v >= 1.0 { "1" } else { "0" })
                .unwrap_or("0");
            out_row.insert("adult_7_9".to_string(), pneu.to_string());
            out_row.insert("a_7_11".to_string(), suic.to_string());
        }

        let record: Vec<&str> = out_headers
            .iter()
            .map(|h| out_row.get(h).map(|s| s.as_str()).unwrap_or(""))
            .collect();
        wtr.write_record(&record)
            .map_err(|e| SmartVaError::CsvError(e.to_string()))?;
    }

    wtr.flush()
        .map_err(|e| SmartVaError::IoError(e.to_string()))?;
    Ok(row_count)
}
