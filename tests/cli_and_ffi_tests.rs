use std::fs;
use std::process::Command;

use smartva_rust::phmrc_source_prep::convert_phmrc_gold_standard_csv;

#[test]
fn test_smartva_rs_cli_help_version_countries_and_json() {
    let bin = env!("CARGO_BIN_EXE_smartva-rs");

    // 1. --version and version subcommand
    for flag in ["--version", "-V", "version"] {
        let out = Command::new(bin).arg(flag).output().expect("Failed to run smartva-rs");
        assert!(out.status.success(), "flag {flag} failed");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("SmartVA-Analyze 3.0.1-rust"), "unexpected version output: {stdout}");
    }

    // 2. --help, -h, help subcommand, and zero-arg invocation
    for args in [vec!["--help"], vec!["-h"], vec!["help"], vec![]] {
        let out = Command::new(bin).args(&args).output().expect("Failed to run smartva-rs");
        assert!(out.status.success(), "help args {args:?} failed");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("USAGE:"), "missing USAGE in help: {stdout}");
        assert!(stdout.contains("COMMANDS:"), "missing COMMANDS in help: {stdout}");
        assert!(stdout.contains("EXAMPLES:"), "missing EXAMPLES in help: {stdout}");
    }

    // 3. countries / --list-countries
    for flag in ["--list-countries", "countries"] {
        let out = Command::new(bin).arg(flag).output().expect("Failed to run smartva-rs");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("India (IND)"));
    }

    // 4. Inline --json scoring with --format json-extended
    let out = Command::new(bin)
        .args([
            "--country",
            "IND",
            "--json",
            r#"{"sid":"CLI-01","Id10013":"yes","Id10019":"male","ageInYears":62,"Id10147":"yes"}"#,
            "--format",
            "json-extended",
        ])
        .output()
        .expect("Failed to run smartva-rs --json");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("CLI-01"), "unexpected json stdout: {stdout}");
    assert!(stdout.contains("csmf.csv"), "missing csmf.csv in extended json: {stdout}");
}

#[test]
fn test_phmrc_source_prep_cli_and_neonate_conversion() {
    let bin = env!("CARGO_BIN_EXE_phmrc-source-prep");

    for flag in ["--version", "-V", "version"] {
        let out = Command::new(bin).arg(flag).output().expect("Failed to run phmrc-source-prep");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("phmrc-source-prep 3.0.2 (SmartVA-Analyze 3.0.1-rust)"));
    }

    for args in [vec!["--help"], vec!["-h"], vec!["help"], vec![]] {
        let out = Command::new(bin).args(&args).output().expect("Failed to run phmrc-source-prep");
        assert!(out.status.success());
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("USAGE:"));
        assert!(stdout.contains("COMMANDS:"));
    }

    // Test synthetic PHMRC Neonate row conversion (including c1_09, c1_26, c2_13a, c3_14=1.0->3.0, and c5_07_1 weight)
    let tmp = tempfile::tempdir().unwrap();
    let in_csv = tmp.path().join("neonate_gs.csv");
    let out_csv = tmp.path().join("neonate_odk.csv");
    fs::write(
        &in_csv,
        "site,module,newid,gs_code34,gs_text34,va34,gs_code46,gs_text46,va46,g1_01,g1_06a,g5_01a,g5_01b,g5_01c,g5_02,g5_03a,g5_03b,g5_03c,g5_04a,g5_04b,g5_04c,c1_09,c1_26,c2_13a,c3_14,c5_07_1,word_cough\n\
         UP,Neonate,42,6,Stillbirth,6,6,Stillbirth,6,1,20,2010,5,10,Female,2010,5,10,0,0,0,Female,Stillbirth,Home,1.0,2500,1\n",
    )
    .unwrap();

    let rows = convert_phmrc_gold_standard_csv(&in_csv, &out_csv).expect("Conversion failed");
    assert_eq!(rows, 1);

    let odk_text = fs::read_to_string(&out_csv).unwrap();
    assert!(odk_text.starts_with("sid,"), "unexpected odk header: {odk_text}");
    assert!(odk_text.contains("\n42,"), "missing sid 42: {odk_text}");
    // Verify Neonate c1_09 Female -> 1.0, c1_26 Stillbirth -> 0, c2_13a Home -> 1, c3_14 1.0 -> 3
    assert!(odk_text.contains(",1.0,0,1,3,"), "unexpected neonate values: {odk_text}");
}
