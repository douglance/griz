//! Manual measurement of restoring long, valid chains of recorded writes.

use crate::common::{Fixture, TestResult, undo_request};
use griz_store::{FileWrite, OnStale, Operation, OperationKind, OperationState, RestoreScope};
use rusqlite::{Connection, params};
use std::{error::Error, hint::black_box, time::Instant};

fn seed(fx: &Fixture, count: usize) -> Result<String, Box<dyn Error>> {
    let hashes = [fx.store.put_blob("zero\n")?, fx.store.put_blob("one\n")?];
    let mut conn = Connection::open(fx.store.home().join("griz.sqlite3"))?;
    let tx = conn.transaction()?;
    let mut insert = tx.prepare(
        "INSERT INTO operations (id, created_at, state, body) VALUES (?1, ?2, 'applied', ?3)",
    )?;
    for index in 0..count {
        let mut op = Operation::new(OperationKind::Apply, "benchmark");
        op.id = format!("op_{:032x}", count - index);
        op.state = OperationState::Applied;
        op.files = vec![FileWrite {
            path: fx.path("a.txt"),
            before_hash: Some(hashes[index % 2].clone()),
            after_hash: Some(hashes[1 - index % 2].clone()),
            merged: false,
        }];
        insert.execute(params![op.id, op.created_at, serde_json::to_string(&op)?])?;
    }
    drop(insert);
    tx.commit()?;
    fx.write("a.txt", "one\n")?;
    Ok(format!("op_{count:032x}"))
}

fn measure(count: usize) -> TestResult {
    let fx = Fixture::new()?;
    let first = seed(&fx, count)?;
    let mut samples = Vec::new();
    for trial in 0..3 {
        let start = Instant::now();
        let restored = black_box(fx.store.restore_since(
            &first,
            RestoreScope {
                root: &fx.root(),
                paths: &[],
            },
            OnStale::Refuse,
            "benchmark",
        )?);
        samples.push(start.elapsed().as_micros());
        assert_eq!(
            restored.state,
            OperationState::Applied,
            "{:?}",
            restored.reason
        );
        assert_eq!(fx.read("a.txt")?, "zero\n");
        assert_eq!(
            restored
                .restores
                .as_ref()
                .ok_or("no span")?
                .operations
                .len(),
            count + 2 * trial
        );
        let redone = fx.store.undo(&undo_request(fx.root(), &restored.id))?;
        assert_eq!(redone.state, OperationState::Applied);
        assert_eq!(fx.read("a.txt")?, "one\n");
    }
    samples.sort_unstable();
    println!("restore count={count} median_us={}", samples[1]);
    Ok(())
}

#[test]
#[ignore = "manual release-mode span restore timing"]
fn restore_span_measurement() -> TestResult {
    for count in [1_001, 10_001, 50_001] {
        measure(count)?;
    }
    Ok(())
}
