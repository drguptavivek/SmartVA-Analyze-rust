#!/usr/bin/env python3
"""Example Python script calling the Pure-Rust SmartVA-Analyze engine (`smartva-rs`).

Note: The Rust crate (`smartva-rust` / `smartva-rs`) is 100% pure native Rust
with ZERO runtime Python dependency. This script is provided purely as an example
of how external Python callers can invoke the compiled Rust CLI (`smartva-rs`) or
C-ABI shared library (`libsmartva_rust`) using:
  1. Directory mode (`--csv True --charts True`)
  2. JSON input -> in-memory CSV -> JSON output (standard individual level & extended cohort level)
  3. JSON input -> in-memory CSV -> Protocol Buffers (`proto3`) binary output
"""

from __future__ import annotations

import csv
import ctypes
import json
import subprocess
import sys
import tempfile
from pathlib import Path

RUST_DIR = Path(__file__).resolve().parents[1]
RELEASE_DIR = RUST_DIR / "target" / "release"


def _find_shared_lib() -> Path:
    for ext in ("dylib", "so", "dll"):
        candidate = RELEASE_DIR / f"libsmartva_rust.{ext}"
        if candidate.exists():
            return candidate
    raise FileNotFoundError(
        f"Shared library libsmartva_rust not found in {RELEASE_DIR}; build with `cargo build --release`."
    )


def run_via_cli(
    input_csv: Path,
    output_dir: Path,
    country: str = "IND",
    write_csv: bool = True,
    write_charts: bool = True,
) -> None:
    """Invoke the drop-in `smartva-rs` binary via subprocess."""
    bin_path = RELEASE_DIR / "smartva-rs"
    if not bin_path.exists():
        raise FileNotFoundError(
            f"smartva-rs binary not found at {bin_path}; build with `cargo build --release` first."
        )
    cmd = [
        str(bin_path),
        "--country",
        country,
        "--hiv",
        "True",
        "--malaria",
        "True",
        "--hce",
        "True",
        "--freetext",
        "True",
        "--csv",
        str(write_csv),
        "--charts",
        str(write_charts),
        str(input_csv),
        str(output_dir),
    ]
    subprocess.run(cmd, check=True)


def run_via_ffi(input_csv: Path, output_dir: Path, country: str = "IND") -> None:
    """Invoke `libsmartva_rust` directory pipeline in-process via C-ABI FFI (`ctypes`)."""
    lib = ctypes.CDLL(str(_find_shared_lib()))
    lib.smartva_warm_cache.argtypes = []
    lib.smartva_warm_cache.restype = ctypes.c_int

    lib.smartva_run_directory.argtypes = [
        ctypes.c_char_p,
        ctypes.c_char_p,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.POINTER(ctypes.c_char_p),
    ]
    lib.smartva_run_directory.restype = ctypes.c_int

    lib.smartva_warm_cache()
    err_ptr = ctypes.c_char_p()
    rc = lib.smartva_run_directory(
        str(input_csv).encode("utf-8"),
        str(output_dir).encode("utf-8"),
        country.encode("utf-8"),
        1,  # hiv
        1,  # malaria
        1,  # hce
        1,  # freetext
        1,  # figures
        ctypes.byref(err_ptr),
    )
    if rc != 0:
        err_msg = err_ptr.value.decode("utf-8") if err_ptr.value else f"FFI exit code {rc}"
        raise RuntimeError(err_msg)


def score_json_via_ffi(payload: dict | list, country: str = "IND", extended: bool = False) -> dict:
    """Score a JSON VA payload 100% in memory via C-ABI `smartva_score_json`.

    - `extended=False` (default): returns individual-level CSV outputs converted to JSON
      (`individual-cause-of-death.csv`, `{module}-predictions.csv`, `{module}-likelihoods.csv`,
      `{module}-tariff-scores.csv`, `{module}-tariff-ranks.csv`, `{module}-external-ranks.csv`).
    - `extended=True`: adds population CSMF tables (`csmf.csv`, `{module}-csmf.csv`,
      `gbd-level1-csmf.csv`, `causes-of-death.csv`, `VA-data-age-groupings.csv`,
      `{module}-endorsement-rates.csv`, and `report`).
    """
    lib = ctypes.CDLL(str(_find_shared_lib()))
    lib.smartva_score_json.argtypes = [
        ctypes.c_char_p,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.POINTER(ctypes.c_void_p),
        ctypes.POINTER(ctypes.c_void_p),
    ]
    lib.smartva_score_json.restype = ctypes.c_int
    lib.smartva_free_string.argtypes = [ctypes.c_void_p]
    lib.smartva_free_string.restype = None

    res_ptr = ctypes.c_void_p()
    err_ptr = ctypes.c_void_p()
    rc = lib.smartva_score_json(
        json.dumps(payload).encode("utf-8"),
        country.encode("utf-8"),
        1,
        1,
        1,
        1,
        1 if extended else 0,
        ctypes.byref(res_ptr),
        ctypes.byref(err_ptr),
    )
    if rc != 0:
        err_msg = (
            ctypes.cast(err_ptr, ctypes.c_char_p).value.decode("utf-8")
            if err_ptr.value
            else f"FFI exit code {rc}"
        )
        if err_ptr.value:
            lib.smartva_free_string(err_ptr)
        raise RuntimeError(err_msg)

    try:
        raw_json = ctypes.cast(res_ptr, ctypes.c_char_p).value.decode("utf-8")
        return json.loads(raw_json)
    finally:
        if res_ptr.value:
            lib.smartva_free_string(res_ptr)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <input.csv> [output_dir] [country]")
        sys.exit(1)

    in_csv = Path(sys.argv[1]).resolve()
    out_dir = (
        Path(sys.argv[2]).resolve()
        if len(sys.argv) >= 3
        else Path(tempfile.mkdtemp(prefix="smartva_rs_example_"))
    )
    iso3 = sys.argv[3] if len(sys.argv) >= 4 else "IND"

    print(f"Running Pure-Rust SmartVA via FFI on {in_csv} -> {out_dir} ...")
    run_via_ffi(in_csv, out_dir, country=iso3)

    pred_csv = out_dir / "1-individual-cause-of-death" / "individual-cause-of-death.csv"
    with pred_csv.open() as f:
        rows = list(csv.DictReader(f))
    print(f"Completed! Scored {len(rows)} VAs into {out_dir}")
