//! All account records below are synthetic fixtures, never local Steam records.
use super::*;
use tempfile::TempDir;

fn user(id: u32, fields: &str) -> String {
    format!("\"{}\" {{ {fields} }}", dpda_domain::steam64(id))
}

fn document(users: &[String]) -> String {
    format!("\"users\" {{ {} }}", users.join(" "))
}

fn root(contents: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("config")).unwrap();
    fs::write(root.path().join("config/loginusers.vdf"), contents).unwrap();
    root
}

fn report(roots: &[&TempDir]) -> DiscoveryReport {
    discover_in_roots(
        &roots
            .iter()
            .map(|r| r.path().to_owned())
            .collect::<Vec<_>>(),
    )
}

#[test]
fn parses_optional_fields_and_only_serializes_public_hints() {
    let input = document(&[user(
        1,
        r#""PersonaName" "测试昵称" "AccountName" "NEVER_EXPORT_LOGIN" "RememberPassword" "1" "Unknown" { "token" "NEVER_EXPORT_TOKEN" }"#,
    )]);
    let candidates = parse_loginusers(input.as_bytes()).unwrap();
    assert_eq!(candidates[0].display_name.as_deref(), Some("测试昵称"));
    assert!(!candidates[0].most_recent);
    assert_eq!(candidates[0].last_used_at, None);
    let serialized = serde_json::to_string(&candidates).unwrap();
    for private in [
        "AccountName",
        "RememberPassword",
        "NEVER_EXPORT",
        "Unknown",
        "token",
    ] {
        assert!(!serialized.contains(private));
    }
    assert!(serialized.contains(&format!("\"steam64\":\"{}\"", dpda_domain::steam64(1))));
}

#[test]
fn handles_utf8_bom_comments_case_unquoted_tokens_and_escapes() {
    let input = format!(
        "\u{feff}// intro\n USERS {{ /* comment */ {} }} // end",
        user(
            2,
            r#"PersonaName "  测试\n\"昵称\"\\path\q  " MOSTRECENT 1 TIMESTAMP 1700000000"#
        )
    );
    let candidate = parse_loginusers(input.as_bytes()).unwrap().remove(0);
    assert_eq!(
        candidate.display_name.as_deref(),
        Some("测试\"昵称\"\\path\\q")
    );
    assert!(candidate.most_recent);
    assert_eq!(candidate.last_used_at, Some(1_700_000_000));
}

#[test]
fn timestamp_is_optional_bounded_and_js_safe() {
    for valid in ["0", "253402300799"] {
        let value =
            parse_loginusers(document(&[user(1, &format!("Timestamp {valid}"))]).as_bytes())
                .unwrap();
        assert_eq!(
            value[0].last_used_at,
            if valid == "0" {
                None
            } else {
                Some(253_402_300_799)
            }
        );
    }
    for invalid in [
        "-1",
        "253402300800",
        "9223372036854775807",
        "1.5",
        "tomorrow",
    ] {
        assert_eq!(
            parse_loginusers(document(&[user(1, &format!("Timestamp {invalid}"))]).as_bytes()),
            Err(ParseError::InvalidField)
        );
    }
}

#[test]
fn validates_individual_steam64_not_steam32_or_other_universes() {
    for key in [
        "1",
        "0",
        "76561197960265728",
        "076561197960265729",
        "76561197960265727",
        "76561202255233024",
        "clan",
    ] {
        assert_eq!(
            parse_loginusers(format!("users {{ \"{key}\" {{}} }}").as_bytes()),
            Err(ParseError::InvalidField)
        );
    }
    assert_eq!(
        parse_loginusers(document(&[user(u32::MAX - 1, "")]).as_bytes()).unwrap()[0].account_id,
        u32::MAX - 1
    );
}

#[test]
fn rejects_duplicate_accounts_and_case_insensitive_fields() {
    assert_eq!(
        parse_loginusers(document(&[user(1, ""), user(1, "")]).as_bytes()),
        Err(ParseError::DuplicateKey)
    );
    for fields in [
        "MostRecent 1 mostrecent 0",
        "Unknown a UNKNOWN b",
        "nested { key a KEY b }",
    ] {
        assert_eq!(
            parse_loginusers(document(&[user(1, fields)]).as_bytes()),
            Err(ParseError::DuplicateKey)
        );
    }
    assert!(parse_loginusers(b"users {} USERS {}").is_err());
}

#[test]
fn rejects_includes_directives_and_malformed_structure_without_raw_error_data() {
    for invalid in [
        "#include /private/file",
        "#base /private/file",
        "users {",
        "users {} trailing",
        "users {} /*",
        "users { \"unterminated",
        "users {} [$WIN32]",
        "users { \"secret_LOGIN\" {} }",
    ] {
        let error = parse_loginusers(invalid.as_bytes()).unwrap_err();
        assert!(!error.to_string().contains("secret_LOGIN"));
        assert!(!format!("{error:?}").contains("/private"));
    }
    assert!(parse_loginusers(document(&[user(1, "MostRecent true")]).as_bytes()).is_err());
    assert!(parse_loginusers(document(&[user(1, "PersonaName {}")]).as_bytes()).is_err());
    assert!(parse_loginusers(b"users {\0}").is_err());
    assert_eq!(parse_loginusers(&[0xff]), Err(ParseError::Encoding));
}

#[test]
fn bounds_files_tokens_fields_depth_and_accounts() {
    assert_eq!(
        parse_loginusers(&vec![b' '; parser::MAX_FILE_BYTES + 1]),
        Err(ParseError::TooLarge)
    );
    let long = format!("PersonaName \"{}\"", "x".repeat(4097));
    assert_eq!(
        parse_loginusers(document(&[user(1, &long)]).as_bytes()),
        Err(ParseError::Limit)
    );
    let fields = (0..10_001).map(|i| format!("f{i} x ")).collect::<String>();
    assert_eq!(
        parse_loginusers(document(&[user(1, &fields)]).as_bytes()),
        Err(ParseError::Limit)
    );
    let deep = format!("{} leaf x {}", "nested { ".repeat(20), "} ".repeat(20));
    assert_eq!(
        parse_loginusers(document(&[user(1, &deep)]).as_bytes()),
        Err(ParseError::Limit)
    );
    let users = (1..=257).map(|i| user(i, "")).collect::<Vec<_>>();
    assert_eq!(
        parse_loginusers(document(&users).as_bytes()),
        Err(ParseError::Limit)
    );
}

#[test]
fn short_arbitrary_bytes_and_unicode_do_not_panic() {
    for value in 0..=255 {
        let _ = parse_loginusers(&[value]);
        let _ = parse_loginusers(&[b'"', value, b'"']);
    }
    for input in [
        "用户 {}",
        "users { 海 {} }",
        "users { \"😀\" {} }",
        "users { /*😀*/ }",
    ] {
        let _ = parse_loginusers(input.as_bytes());
    }
}

#[test]
fn distinguishes_absent_installation_and_empty_records() {
    let temp = TempDir::new().unwrap();
    assert_eq!(
        discover_in_roots(&[temp.path().join("missing")]).status,
        DiscoveryStatus::NotInstalled
    );
    assert_eq!(report(&[&temp]).status, DiscoveryStatus::NoRecords);
    assert_eq!(
        report(&[&root("users {}")]).status,
        DiscoveryStatus::NoRecords
    );
    assert_eq!(discover_in_roots(&[]).locations_checked, 0);
}

#[test]
fn unique_candidate_can_be_suggested_without_most_recent() {
    let first = root(&document(&[user(1, "PersonaName Synthetic")]));
    let result = report(&[&first]);
    assert_eq!(result.status, DiscoveryStatus::Found);
    assert_eq!(result.suggested_account_id, Some(1));
    assert_eq!(result.locations_checked, 1);
}

#[test]
fn multiple_accounts_without_recent_are_not_chosen_by_timestamp() {
    let records = root(&document(&[
        user(1, "Timestamp 10"),
        user(2, "Timestamp 20"),
    ]));
    let result = report(&[&records]);
    assert_eq!(result.suggested_account_id, None);
    assert_eq!(result.candidates[0].account_id, 2);
}

#[test]
fn unique_recent_is_suggested_but_conflicting_recent_is_only_a_warning() {
    let first = root(&document(&[
        user(1, "MostRecent 1"),
        user(2, "MostRecent 0"),
    ]));
    assert_eq!(report(&[&first]).suggested_account_id, Some(1));
    let second = root(&document(&[user(2, "MostRecent 1")]));
    let result = report(&[&first, &second]);
    assert_eq!(result.status, DiscoveryStatus::Found);
    assert_eq!(result.suggested_account_id, None);
    assert_eq!(
        result.diagnostics[0].code,
        DiagnosticCode::ConflictingMostRecent
    );
}

#[test]
fn merges_duplicate_accounts_deterministically_across_installations() {
    let first = root(&document(&[user(
        1,
        "PersonaName Older Timestamp 10 MostRecent 1",
    )]));
    let second = root(&document(&[user(1, "PersonaName Newer Timestamp 20")]));
    let result = report(&[&first, &second]);
    assert_eq!(result, report(&[&second, &first]));
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].display_name.as_deref(), Some("Newer"));
    assert!(result.candidates[0].most_recent);
    let conflicting = root(&document(&[user(1, "PersonaName Different Timestamp 20")]));
    assert_eq!(
        report(&[&second, &conflicting]).candidates[0].display_name,
        None
    );
}

#[test]
fn a_failed_location_suppresses_automatic_selection_and_reports_partial() {
    let valid = root(&document(&[user(1, "MostRecent 1")]));
    let invalid = root("users { broken");
    let result = report(&[&valid, &invalid]);
    assert_eq!(result.status, DiscoveryStatus::Partial);
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.suggested_account_id, None);
    assert_eq!(report(&[&invalid]).status, DiscoveryStatus::InvalidFormat);
    assert!(!serde_json::to_string(&result.diagnostics)
        .unwrap()
        .contains(invalid.path().to_str().unwrap()));
}

#[test]
fn relative_roots_and_excessive_root_counts_are_bounded() {
    let result = discover_in_roots(&[PathBuf::from("relative")]);
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnsafePath);
    assert_eq!(result.locations_checked, 0);
    let temp = TempDir::new().unwrap();
    let result = discover_in_roots(&vec![temp.path().to_owned(); 33]);
    assert_eq!(result.locations_checked, 1);
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::LimitExceeded);
}

#[test]
fn filesystem_size_limit_and_permission_diagnostics_are_explicit() {
    let huge = root(&" ".repeat(parser::MAX_FILE_BYTES + 1));
    assert_eq!(
        report(&[&huge]).diagnostics[0].code,
        DiagnosticCode::LimitExceeded
    );
    let permission = io_diagnostic(&io::Error::from(io::ErrorKind::PermissionDenied), "fixture");
    assert_eq!(permission.code, DiagnosticCode::PermissionDenied);
    let report = assemble(vec![], vec![permission]);
    assert_eq!(report.status, DiscoveryStatus::PermissionDenied);
}

#[cfg(unix)]
#[test]
fn installation_root_symlinks_are_supported_and_deduplicated() {
    let actual = root(&document(&[user(1, "")]));
    let parent = TempDir::new().unwrap();
    let alias = parent.path().join("steam-alias");
    std::os::unix::fs::symlink(actual.path(), &alias).unwrap();
    let result = discover_in_roots(&[actual.path().to_owned(), alias]);
    assert_eq!(result.locations_checked, 1);
    assert_eq!(result.suggested_account_id, Some(1));
}

#[cfg(unix)]
#[test]
fn config_and_record_symlinks_are_rejected_without_reading_target() {
    let outside = root(&document(&[user(2, "")]));
    let linked_config = TempDir::new().unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("config"),
        linked_config.path().join("config"),
    )
    .unwrap();
    let linked_file = TempDir::new().unwrap();
    fs::create_dir(linked_file.path().join("config")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("config/loginusers.vdf"),
        linked_file.path().join("config/loginusers.vdf"),
    )
    .unwrap();
    for invalid in [&linked_config, &linked_file] {
        let result = report(&[invalid]);
        assert!(result.candidates.is_empty());
        assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnsafePath);
    }
}

#[cfg(unix)]
#[test]
fn a_named_pipe_cannot_block_or_supply_record_contents() {
    use std::os::unix::ffi::OsStrExt;
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join("config")).unwrap();
    let path = std::ffi::CString::new(
        temp.path()
            .join("config/loginusers.vdf")
            .as_os_str()
            .as_bytes(),
    )
    .unwrap();
    // SAFETY: path is a NUL-terminated path inside this temporary fixture.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let result = report(&[&temp]);
    assert!(result.candidates.is_empty());
    assert_eq!(result.diagnostics[0].code, DiagnosticCode::UnsafePath);
}

#[cfg(windows)]
#[test]
fn windows_roots_reject_network_and_device_paths_before_io() {
    for path in [
        r"\\server\share\Steam",
        r"\\?\UNC\server\share\Steam",
        r"\\.\PIPE\steam",
        r"C:Steam",
        r"\Steam",
    ] {
        assert!(!local_absolute_root(Path::new(path)), "{path}");
    }
    for path in [r"C:\Steam", r"\\?\C:\Steam"] {
        assert!(local_absolute_root(Path::new(path)), "{path}");
    }
}
