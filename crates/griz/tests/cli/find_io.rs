//! A failed search scope must not satisfy a match-count assertion.

use crate::common::{Griz, TestResult};

#[test]
fn pattern_paths_report_the_glob_option() -> TestResult {
    let griz = Griz::new()?;
    griz.write("src/a.rs", "// needle\n")?;
    for mode in [&[][..], &["--files-only"][..]] {
        let mut args = vec![
            "find",
            "--paths",
            "src/*.rs",
            "--literal",
            "needle",
            "--expect-matches",
            "1",
        ];
        args.extend(mode);
        let run = griz.run(&args)?;
        assert_ne!(run.code, Some(0), "{}", run.json);
        assert_eq!(run.json["code"], "VALIDATION_ERROR");
        let message = run.json["message"].as_str().ok_or("no message")?;
        assert!(message.contains("--glob"), "{message}");
        assert!(message.contains("MCP: glob"), "{message}");
        assert!(run.json.get("matches").is_none());
        assert!(run.json.get("file_matches").is_none());
    }
    Ok(())
}

#[test]
fn pattern_paths_that_exist_remain_literal() -> TestResult {
    let griz = Griz::new()?;
    griz.write("src/[ab].rs", "// needle\n")?;
    griz.write("src/a.rs", "// needle needle\n")?;
    let run = griz.run(&[
        "find",
        "--paths",
        "src/[ab].rs",
        "--literal",
        "needle",
        "--expect-matches",
        "1",
    ])?;
    assert_eq!(run.code, Some(0), "{}", run.json);
    assert_eq!(run.json["total"], 1);
    let path = run.json["matches"][0]["path"].as_str().ok_or("no path")?;
    assert!(path.ends_with("[ab].rs"), "{path}");
    Ok(())
}

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
    assert!(
        !plain.json["message"]
            .as_str()
            .ok_or("no message")?
            .contains("--glob")
    );
    Ok(())
}
