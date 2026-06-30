//! Shared helpers for the integration tests (golden-vault fixtures).
#![allow(dead_code)]

use b2_core::id::IdGen;
use std::fs;
use std::path::Path;

/// b2ids of the two golden-vault notes (planning/data-model.md §8).
pub const MEMORY_ID: &str = "01JMEM0000000000000000000A";
pub const SRS_ID: &str = "01JSRS0000000000000000000B";

/// Deterministic id generator so stamping / suggestion ids are assertable.
pub struct FixedId(pub &'static str);
impl IdGen for FixedId {
    fn new_id(&self) -> String {
        self.0.to_string()
    }
}

pub fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// Copy the committed golden vault into `dst` so ingest (which may stamp a
/// `b2id`) never mutates the repo fixtures.
pub fn golden_vault_copy(dst: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden-vault");
    copy_dir(&src, dst);
}
