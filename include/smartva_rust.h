#ifndef SMARTVA_RUST_H
#define SMARTVA_RUST_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Pre-warm the in-process Tariff 2.0 validation matrices (Adult, Child, Neonate).
 * Returns 0 on success, -1 on failure.
 */
int smartva_warm_cache(void);

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
