#ifndef SMARTVA_RUST_H
#define SMARTVA_RUST_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Panic safety: every function below catches Rust panics instead of aborting the
 * calling process. Int-returning functions then return non-zero (smartva_warm_cache: -1;
 * the others: 2) and, where an `error_out` parameter exists, set `*error_out` to a message
 * starting with "smartva-rust panicked:". smartva_score_json_simple returns {"error":"..."}.
 */

/**
 * Version banner, e.g. "SmartVA-Analyze 3.0.1-rust (smartva-rust 3.0.2)".
 * Static NUL-terminated string, same text as `smartva-rs --version`. Do NOT free.
 */
const char *smartva_version(void);

/**
 * Pre-warm the in-process Tariff 2.0 validation matrices (Adult, Child, Neonate) for the
 * PHMRC full instrument with HCE and free text. WHO 2016/2022 input uses the short-form
 * models: warm those with smartva_warm_cache_for(1, 1, 1).
 * Returns 0 on success, -1 on failure.
 */
int smartva_warm_cache(void);

/**
 * Pre-warm the models (Adult, Child, Neonate) one input type uses: short_form = 1 for
 * WHO 2016/2022 input, 0 for PHMRC full; hce and free_text as passed when scoring.
 * Country, HIV and malaria are applied per run and need no warming.
 * Returns 0 on success, -1 on failure.
 */
int smartva_warm_cache_for(int hce, int free_text, int short_form);

/**
 * Run the full SmartVA-Analyze pipeline on `input_csv_path` and write the 4-folder
 * output hierarchy (CSVs, .xlsx workbooks, and optional .png figures) into `output_dir_path`.
 * Returns 0 on success, non-zero on failure (writes error C-string to `*error_out` if non-null).
 */
int smartva_run_directory(
    const char *input_csv_path,
    const char *output_dir_path,
    const char *country,
    int hiv,
    int malaria,
    int hce,
    int freetext,
    int figures,
    char **error_out
);

/**
 * Score a JSON Verbal Autopsy payload (single VA object, array of VAs, or {"records":[...]})
 * in memory and return a newly allocated UTF-8 JSON C-string in `*result_json_out`.
 * - Set `extended = 0` for individual predictions & top-3 likelihoods only.
 * - Set `extended = 1` to also include cohort CSMFs, age groupings, and quality report.
 * Returns 0 on success, non-zero on failure. Free `*result_json_out` / `*error_out` with `smartva_free_string`.
 */
int smartva_score_json(
    const char *json_payload,
    const char *country,
    int hiv,
    int malaria,
    int hce,
    int freetext,
    int extended,
    char **result_json_out,
    char **error_out
);

/**
 * Simplified single-return-pointer C-ABI helper for Swift, Flutter (`dart:ffi`),
 * Kotlin (JNA), Node.js (`koffi`), Bun (`bun:ffi`), Deno (`Deno.dlopen`), and Python (`ctypes`).
 * Returns a newly allocated UTF-8 JSON C-string directly (or `{"error":"..."}` on failure).
 * Always free the returned pointer with `smartva_free_string`.
 */
char *smartva_score_json_simple(
    const char *json_payload,
    const char *country,
    int hiv,
    int malaria,
    int hce,
    int freetext,
    int extended
);

/**
 * Score a JSON Verbal Autopsy payload in memory and return a newly allocated Protocol Buffers
 * (`proto3` wire format, see `proto/smartva.proto`) byte buffer in `*result_bytes_out` and `*result_len_out`.
 * Free with `smartva_free_bytes`.
 */
int smartva_score_protobuf(
    const char *json_payload,
    const char *country,
    int hiv,
    int malaria,
    int hce,
    int freetext,
    int extended,
    uint8_t **result_bytes_out,
    size_t *result_len_out,
    char **error_out
);

/**
 * Free a C-string allocated by `smartva_run_directory`, `smartva_score_json`, or `smartva_score_json_simple`.
 */
void smartva_free_string(char *ptr);

/**
 * Free a byte buffer allocated by `smartva_score_protobuf`.
 */
void smartva_free_bytes(uint8_t *ptr, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* SMARTVA_RUST_H */
