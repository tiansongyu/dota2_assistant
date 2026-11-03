use std::process::Command;

#[test]
fn steam_discovery_reads_only_explicit_fixture_and_preserves_steam64() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config");
    std::fs::create_dir(&config).unwrap();
    let vdf = r#""users" {
        "76561198030654385" {
            "PersonaName" "DPDA synthetic account"
            "AccountName" "MUST_NOT_EXPORT_LOGIN_NAME"
            "RememberPassword" "MUST_NOT_EXPORT_REMEMBER_PASSWORD"
            "MostRecent" "1"
            "Timestamp" "1700000000"
        }
    }"#;
    std::fs::write(config.join("loginusers.vdf"), vdf).unwrap();
    std::fs::write(config.join("config.vdf"), "MUST_NOT_READ_OTHER_FILES").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dpda-debug"))
        .args(["steam-accounts", "--steam-root"])
        .arg(".")
        .current_dir(directory.path())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "found");
    assert_eq!(report["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(report["candidates"][0]["steam64"], "76561198030654385");
    assert_eq!(report["suggested_account_id"], 70388657);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("MUST_NOT"));
    assert!(output.stderr.is_empty());
    assert_eq!(
        std::fs::read_to_string(config.join("loginusers.vdf")).unwrap(),
        vdf
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn invalid_steam_file_fails_without_echoing_its_content() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(
        config.join("loginusers.vdf"),
        "MUST_NOT_EXPORT_INVALID_CONTENT {",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dpda-debug"))
        .args(["steam-accounts", "--steam-root"])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "invalid_format");
    assert!(report["candidates"].as_array().unwrap().is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("MUST_NOT_EXPORT"));
    assert!(output.stderr.is_empty());
}

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
