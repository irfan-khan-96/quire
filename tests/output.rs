//! quire's stdout may be piped into tools that stop reading early.

use std::process::{Command, Stdio};

#[test]
fn closed_stdout_does_not_panic() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quire"))
        .arg("doctor")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run quire");
    // Close the read end before doctor prints anything, like `quire doctor | head -0`.
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("wait for quire");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "quire panicked:\n{stderr}");
    assert!(output.status.success(), "quire exited with {:?}:\n{stderr}", output.status);
}
