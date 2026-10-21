//! Native Rust `.xlsx` workbook generator using [`rust_xlsxwriter`].
//!
//! Replaces Python's `xlsxwriter` dependency in `TariffPrep.write_multiple_predictions_xlsx`,
//! producing `4-monitoring-and-quality/{adult,child,neonate}-likelihoods.xlsx` with
//! identical column widths, frozen panes, row heights, and likelihood color fills.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rust_xlsxwriter::{Color, Format, FormatAlign, Workbook};

const LIKELIHOOD_COLORS: &[(&str, u32)] = &[
    ("Very Likely", 0x3CB371),
    ("Likely", 0x47D147),
    ("Somewhat Likely", 0x8AE600),
    ("Possible", 0xE6E600),
    // Chinese & Spanish likelihood labels
    ("非常可能", 0x3CB371),
    ("可能", 0x47D147),
    ("有点可能", 0x8AE600),
    ("或许可能", 0xE6E600),
    ("Muy probable", 0x3CB371),
    ("Probable", 0x47D147),
    ("Algo probable", 0x8AE600),
    ("Posible", 0xE6E600),
];

fn likelihood_color(label: &str) -> Option<u32> {
    LIKELIHOOD_COLORS
        .iter()
        .find(|(k, _)| *k == label)
        .map(|(_, c)| *c)
}

/// Generate `{adult,child,neonate}-likelihoods.xlsx` inside `4-monitoring-and-quality/`
/// from the intermediate `{module}-likelihoods.csv` files using `rust_xlsxwriter`.
pub fn generate_likelihood_xlsx_files(output_dir: &Path) -> Result<usize> {
    let inter_dir = output_dir
        .join("4-monitoring-and-quality")
        .join("intermediate-files");
    let folder4 = output_dir.join("4-monitoring-and-quality");
    fs::create_dir_all(&folder4).context("Failed to create 4-monitoring-and-quality")?;

    let mut written = 0usize;
    for module in ["adult", "child", "neonate"] {
        let csv_path = inter_dir.join(format!("{module}-likelihoods.csv"));
        if !csv_path.exists() {
            continue;
        }
        let xlsx_path = folder4.join(format!("{module}-likelihoods.xlsx"));
        write_single_likelihood_xlsx(&csv_path, &xlsx_path)?;
        written += 1;
    }
    Ok(written)
}

fn clean_excel_str(s: &str) -> String {
    let mut cleaned: String = s.chars().filter(|&c| c != '\0').collect();
    if cleaned.chars().count() > 32_760 {
        cleaned = cleaned.chars().take(32_760).collect();
    }
    cleaned
}

pub fn write_single_likelihood_xlsx(csv_path: &Path, xlsx_path: &Path) -> Result<()> {
    let raw_bytes =
        fs::read(csv_path).with_context(|| format!("Failed to read {}", csv_path.display()))?;
    let clean_bytes: Vec<u8> = raw_bytes.into_iter().filter(|&b| b != 0).collect();
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(clean_bytes.as_slice());

    let headers: Vec<String> = reader
        .headers()
        .context("Failed to read CSV headers")?
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    let bold_fmt = Format::new().set_bold();
    let text_wrap_fmt = Format::new().set_text_wrap();
    let text_wrap_vcenter_fmt = Format::new()
        .set_text_wrap()
        .set_align(FormatAlign::VerticalCenter);
    let vcenter_fmt = Format::new().set_align(FormatAlign::VerticalCenter);

    worksheet.set_freeze_panes(1, 1)?;
    worksheet.set_column_width(0, 41.00)?;
    for c in 0..3u16 {
        let j = 3 + c * 3;
        worksheet.set_column_width(j, 21.86)?;
        worksheet.set_column_width(j + 1, 10.29)?;
        worksheet.set_column_width(j + 2, 27.29)?;
    }
    worksheet.set_column_width(12, 51.29)?;

    for (col_idx, h) in headers.iter().enumerate() {
        worksheet.write_string_with_format(0, col_idx as u16, h, &bold_fmt)?;
    }

    for (row_offset, result) in reader.records().enumerate() {
        let record = result?;
        let row_idx = (row_offset + 1) as u32;
        worksheet.set_row_height(row_idx, 52.50)?;

        let sid = clean_excel_str(record.get(0).unwrap_or(""));
        let age_str = clean_excel_str(record.get(1).unwrap_or(""));
        let sex = clean_excel_str(record.get(2).unwrap_or(""));

        worksheet.write_string_with_format(row_idx, 0, &sid, &vcenter_fmt)?;
        if let Ok(age_num) = age_str.parse::<f64>()
            && age_num.is_finite()
        {
            worksheet.write_number_with_format(row_idx, 1, age_num, &vcenter_fmt)?;
        } else {
            worksheet.write_string_with_format(row_idx, 1, &age_str, &vcenter_fmt)?;
        }
        worksheet.write_string_with_format(row_idx, 2, &sex, &vcenter_fmt)?;

        for c in 0..3usize {
            let j = 3 + c * 3;
            let cause_str = clean_excel_str(record.get(j).unwrap_or(""));
            let like_str = clean_excel_str(record.get(j + 1).unwrap_or(""));
            let symp_str = clean_excel_str(record.get(j + 2).unwrap_or(""));

            if let Some(rgb) = likelihood_color(&like_str) {
                let fill_fmt = Format::new()
                    .set_background_color(Color::RGB(rgb))
                    .set_text_wrap()
                    .set_align(FormatAlign::VerticalCenter);
                worksheet.write_string_with_format(row_idx, j as u16, &cause_str, &fill_fmt)?;
                worksheet.write_string_with_format(
                    row_idx,
                    (j + 1) as u16,
                    &like_str,
                    &fill_fmt,
                )?;
            } else if !cause_str.is_empty() {
                worksheet.write_string_with_format(row_idx, j as u16, &cause_str, &vcenter_fmt)?;
                worksheet.write_string_with_format(
                    row_idx,
                    (j + 1) as u16,
                    &like_str,
                    &vcenter_fmt,
                )?;
            }
            if !symp_str.is_empty() {
                worksheet.write_string_with_format(
                    row_idx,
                    (j + 2) as u16,
                    &symp_str,
                    &text_wrap_vcenter_fmt,
                )?;
            }
        }

        let all_symp_raw = clean_excel_str(record.get(12).unwrap_or(""));
        let bulleted = clean_excel_str(&if all_symp_raw.trim().is_empty() {
            String::new()
        } else {
            all_symp_raw
                .split("; ")
                .map(|s| format!("\u{2022} {s}"))
                .collect::<Vec<_>>()
                .join("\r\n")
        });
        worksheet.write_string_with_format(row_idx, 12, &bulleted, &text_wrap_fmt)?;
    }

    workbook
        .save(xlsx_path)
        .with_context(|| format!("Failed to save {}", xlsx_path.display()))?;
    Ok(())
}
