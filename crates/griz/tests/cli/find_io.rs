//! A failed search scope must not satisfy a match-count assertion.

use crate::common::{Griz, TestResult};

#[test]
fn missing_search_paths_fail_before_match_count_verdicts() -> TestResult {
    let griz = Griz::new()?;
    griz.write("good.txt", "needle")?;
    for expected in ["0", "1"] {
        let run = griz.run(&[
            "find",
            "--paths",
            "good.txt",
            "--paths",
            "missing",
            "--literal",
            "needle",
            "--expect-matches",
            expected,
        ])?;
        assert_ne!(run.code, Some(0), "{}", run.json);
        assert_eq!(run.json["code"], "VALIDATION_ERROR", "{}", run.json);
        assert!(
            run.json["message"]
                .as_str()
                .is_some_and(|s| s.contains("missing"))
        );
        assert!(run.json.get("matches").is_none(), "{}", run.json);
    }
    let plain = griz.run(&["find", "--paths", "missing", "--literal", "needle"])?;
    assert_ne!(plain.code, Some(0), "{}", plain.json);
    assert_eq!(plain.json["code"], "VALIDATION_ERROR");
    Ok(())
}
