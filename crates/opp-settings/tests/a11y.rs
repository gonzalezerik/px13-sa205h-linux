// SPDX-License-Identifier: AGPL-3.0-or-later
//! The static accessibility checks are part of `cargo test`: a widget
//! that cannot announce itself should fail the build, not a review.
//! This runs the same script anyone can run by hand from the root of
//! the workspace.

use std::path::Path;
use std::process::Command;

#[test]
fn static_accessibility_checks_pass() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/a11y/check.sh");
    let run = Command::new("sh")
        .arg(&script)
        .output()
        .unwrap_or_else(|error| panic!("{} does not run: {error}", script.display()));
    assert!(
        run.status.success(),
        "accessibility check failed:\n{}",
        String::from_utf8_lossy(&run.stdout)
    );
}
