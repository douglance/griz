//! `find --within` on the CLI: scoped matches, default output unchanged.

use crate::common::{Griz, TestResult};

#[test]
fn within_comment_narrows_matches_and_default_shape_is_unchanged() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "// word here\nfn word() {}\n")?;
    let plain = griz.run(&["find", "--literal", "word"])?;
    assert_eq!(plain.json["total"], 2, "{}", plain.json);
    let scoped = griz.run(&["find", "--literal", "word", "--within", "comment"])?;
    assert_eq!(scoped.json["total"], 1, "{}", scoped.json);
    assert_eq!(scoped.json["matches"][0]["line"], 1, "{}", scoped.json);
    Ok(())
}

#[test]
fn an_unknown_within_kind_is_a_validation_error() -> TestResult {
    let griz = Griz::new()?;
    griz.write("a.rs", "fn f() {}\n")?;
    let run = griz.run(&["find", "--literal", "f", "--within", "bogus_kind_xyz"])?;
    assert_eq!(run.json["code"], "VALIDATION_ERROR", "{}", run.json);
    Ok(())
}
