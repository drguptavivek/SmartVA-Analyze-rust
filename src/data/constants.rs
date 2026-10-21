//! Direct Rust translation of `smartva/data/constants.py`.
#![allow(dead_code)]

pub const MALE: i32 = 1;
pub const FEMALE: i32 = 2;

pub const NO: i32 = 0;
pub const YES: i32 = 1;
pub const REFUSED_TO_ANSWER: i32 = 8;
pub const UNKNOWN: i32 = 9;

pub const SEX: &str = "g5_02";
pub const AGE: &str = "g5_04a";
pub const INJURY_DURATION_CUTTOFF: f64 = 30.0;
pub const MATERNAL_AGE_LOWER: f64 = 12.0;
pub const MATERNAL_AGE_UPPER: f64 = 49.0;
pub const PERIOD_OVERDUE_CUTTOFF: f64 = 90.0;
pub const PROLONGED_DELIVERY_CUTTOFF: f64 = 1.0;
