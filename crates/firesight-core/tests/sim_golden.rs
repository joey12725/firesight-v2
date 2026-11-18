//! Golden headline snapshots of the three demo households.
//!
//! `tests/golden/<slug>.json` holds the `SummaryDigest` of 1,000 paths at seed 2026.
//! Any change to the model, the data or a random stream's draw order changes them;
//! that is the point. After a deliberate change, regenerate and review the diff:
//!
//! ```text
//! FIRESIGHT_UPDATE_GOLDEN=1 cargo test -p firesight-core --test sim_golden
//! ```

mod common;

use std::path::PathBuf;

use common::*;
use firesight_core::model::Profile;

const SEED: u64 = 2026;
const PATHS: u64 = 1000;

#[test]
fn demo_households_match_their_golden_digests() {
    let update = std::env::var_os("FIRESIGHT_UPDATE_GOLDEN").is_some();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut stale = Vec::new();
    for example in Profile::examples(START) {
        let plan = compile(&example.profile);
        let digest = plan.summarize(&run(&plan, SEED, 0..PATHS)).digest();
        let text = serde_json::to_string_pretty(&digest).unwrap() + "\n";
        let file = dir.join(format!("{}.json", example.slug));
        if update {
            std::fs::write(&file, &text).unwrap();
            continue;
        }
        let golden = std::fs::read_to_string(&file).unwrap_or_default();
        if golden != text {
            stale.push(format!(
                "{}: expected (golden)\n{golden}\nactual\n{text}",
                example.slug
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "golden digests differ; if the change is intended, regenerate with \
         FIRESIGHT_UPDATE_GOLDEN=1 cargo test -p firesight-core --test sim_golden\n{}",
        stale.join("\n")
    );
}
