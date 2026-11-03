use std::process::Command;

#[test]
fn doctor_runs_without_a_display_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_dpda-debug"))
        .arg("doctor")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["runtime_requirements"]["gtk"], false);
    assert_eq!(report["runtime_requirements"]["opencv"], false);
    assert_eq!(report["sqlite"]["status"], "ok");
}

#[test]
fn invalid_steam_id_never_creates_a_database() {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("test.db");
    let output = Command::new(env!("CARGO_BIN_EXE_dpda-debug"))
        .args(["sync", "--account", "1e9", "--from", "2026-01-01", "--db"])
        .arg(&db)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!db.exists());
}
