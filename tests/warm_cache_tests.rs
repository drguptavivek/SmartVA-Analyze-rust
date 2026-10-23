// Own test binary: the training cache is process-wide, so no other test may run beside this one.

use smartva_rust::engine::training_cache_len;
use smartva_rust::{SmartVaEngine, SmartVaRunOptions, smartva_warm_cache_for};

#[test]
fn test_warmed_who_2022_run_builds_no_new_model() {
    assert_eq!(training_cache_len(), 0);
    assert_eq!(smartva_warm_cache_for(1, 1, 1), 0);
    assert_eq!(training_cache_len(), 3, "one short-form model per age group");

    // WHO 2016/2022 field names select the short-form models.
    let payload = r#"{"sid": "VA-WARM-001", "Id10019": "male", "ageInYears": 62,
                      "Id10147": "yes", "Id10148": 14, "Id10153": "yes", "Id10154": 5}"#;
    let opts = SmartVaRunOptions {
        country: "IND".to_string(),
        hce: true,
        freetext: true,
        ..SmartVaRunOptions::default()
    };
    let out = SmartVaEngine::new().score_json(payload, &opts).expect("scoring failed");
    assert!(out.contains("VA-WARM-001"), "{out}");
    assert_eq!(training_cache_len(), 3, "the run built a model the warm-up missed");
}
