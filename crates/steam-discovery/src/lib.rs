//! Read-only local Steam account hints. This module never authenticates a user.
//! It opens only `<known Steam root>/config/loginusers.vdf`, never credentials.
mod parser;
#[cfg(test)]
mod tests;
pub use parser::{parse_loginusers, ParseError};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub account_id: u32,
    pub steam64: String,
    pub display_name: Option<String>,
    pub most_recent: bool,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStatus {
    /// All discovered records were readable. Ambiguity warnings may still exist.
    Found,
    /// Some locations could not be read or parsed; no account is auto-suggested.
    Partial,
    NotInstalled,
    NoRecords,
    PermissionDenied,
    InvalidFormat,
    IoError,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    PermissionDenied,
    InvalidFormat,
    LimitExceeded,
    ReadFailed,
    UnsafePath,
    NoHomeDirectory,
    ConflictingMostRecent,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    /// A fixed source label, never an OS username, install path, or VDF value.
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub status: DiscoveryStatus,
    pub candidates: Vec<Candidate>,
    pub suggested_account_id: Option<u32>,
    pub diagnostics: Vec<Diagnostic>,
    pub locations_checked: u32,
}

fn diagnostic(code: DiagnosticCode, source: &str) -> Diagnostic {
    let message = match code {
        DiagnosticCode::PermissionDenied => "无权读取本机 Steam 账号记录，可手动输入账号 ID",
        DiagnosticCode::InvalidFormat => "本机 Steam 账号记录格式不兼容，未从该文件选择账号",
        DiagnosticCode::LimitExceeded => "本机 Steam 账号记录超过安全读取限制",
        DiagnosticCode::ReadFailed => "本机 Steam 账号记录读取失败，可稍后重试或手动输入",
        DiagnosticCode::UnsafePath => "跳过非普通文件、链接配置文件或非绝对安装路径",
        DiagnosticCode::NoHomeDirectory => "无法确定当前用户目录，可手动输入账号 ID",
        DiagnosticCode::ConflictingMostRecent => "多个账号被标为最近使用，请选择需要分析的账号",
        DiagnosticCode::UnsupportedPlatform => "当前系统尚未实现 Steam 本机账号发现",
    };
    Diagnostic {
        code,
        source: source.into(),
        message: message.into(),
    }
}

fn io_diagnostic(error: &io::Error, source: &str) -> Diagnostic {
    diagnostic(
        if error.kind() == io::ErrorKind::PermissionDenied {
            DiagnosticCode::PermissionDenied
        } else {
            DiagnosticCode::ReadFailed
        },
        source,
    )
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(unix)]
fn open_record(root: &Path) -> Result<Option<File>, ReadError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    let root = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(ReadError::Io)?;
    let open_child = |parent: &File, name: &std::ffi::CStr, directory: bool| {
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if directory {
                libc::O_DIRECTORY
            } else {
                libc::O_NONBLOCK
            };
        // SAFETY: parent owns a live fd; name is a fixed, NUL-terminated basename.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                return Ok(None);
            }
            if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) {
                return Err(ReadError::Unsafe);
            }
            return Err(ReadError::Io(error));
        }
        // SAFETY: openat returned a new owned fd, transferred exactly once.
        Ok(Some(unsafe { File::from_raw_fd(fd) }))
    };
    let Some(config) = open_child(&root, c"config", true)? else {
        return Ok(None);
    };
    open_child(&config, c"loginusers.vdf", false)
}

#[cfg(windows)]
fn open_record(root: &Path) -> Result<Option<File>, ReadError> {
    use std::os::windows::fs::OpenOptionsExt;
    let open = |path: &Path, directory: bool| {
        // Reparse points are opened themselves. Excluding FILE_SHARE_DELETE pins
        // both parent directories until the account file has been opened.
        let flags = 0x0020_0000 | if directory { 0x0200_0000 } else { 0 };
        let file = match fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2 | if directory { 0 } else { 0x4 })
            .custom_flags(flags)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ReadError::Io(error)),
        };
        let metadata = file.metadata().map_err(ReadError::Io)?;
        if is_link(&metadata) || (directory && !metadata.is_dir()) {
            return Err(ReadError::Unsafe);
        }
        Ok(Some(file))
    };
    let Some(_root) = open(root, true)? else {
        return Ok(None);
    };
    let config_path = root.join("config");
    let Some(_config) = open(&config_path, true)? else {
        return Ok(None);
    };
    open(&config_path.join("loginusers.vdf"), false)
}

#[cfg(not(any(unix, windows)))]
fn open_record(_root: &Path) -> Result<Option<File>, ReadError> {
    Err(ReadError::Unsafe)
}

fn read_candidates(root: &Path) -> Result<Option<Vec<Candidate>>, ReadError> {
    let Some(file) = open_record(root)? else {
        return Ok(None);
    };
    let metadata = file.metadata().map_err(ReadError::Io)?;
    if is_link(&metadata) || !metadata.is_file() {
        return Err(ReadError::Unsafe);
    }
    if metadata.len() > parser::MAX_FILE_BYTES as u64 {
        return Err(ReadError::Parse(ParseError::TooLarge));
    }
    let mut bytes = Vec::new();
    file.take(parser::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(ReadError::Io)?;
    parse_loginusers(&bytes).map(Some).map_err(ReadError::Parse)
}

enum ReadError {
    Io(io::Error),
    Parse(ParseError),
    Unsafe,
}

fn local_absolute_root(root: &Path) -> bool {
    if !root.is_absolute() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        matches!(root.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn assemble(
    roots: Vec<(PathBuf, &'static str)>,
    mut diagnostics: Vec<Diagnostic>,
) -> DiscoveryReport {
    let mut unique_roots = BTreeSet::new();
    let mut accounts: BTreeMap<u32, Vec<Candidate>> = BTreeMap::new();
    let mut locations_checked = 0;
    let mut found_root = false;
    if roots.len() > 32 {
        diagnostics.push(diagnostic(DiagnosticCode::LimitExceeded, "root_list"));
    }
    for (root, source) in roots.into_iter().take(32) {
        if !local_absolute_root(&root) {
            diagnostics.push(diagnostic(DiagnosticCode::UnsafePath, source));
            continue;
        }
        let canonical = match fs::canonicalize(&root) {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                diagnostics.push(io_diagnostic(&error, source));
                continue;
            }
        };
        if !local_absolute_root(&canonical) {
            diagnostics.push(diagnostic(DiagnosticCode::UnsafePath, source));
            continue;
        }
        if !unique_roots.insert(canonical.clone()) {
            continue;
        }
        found_root = true;
        locations_checked += 1;
        match read_candidates(&canonical) {
            Ok(Some(candidates)) => {
                for candidate in candidates {
                    accounts
                        .entry(candidate.account_id)
                        .or_default()
                        .push(candidate);
                }
            }
            Ok(None) => (),
            Err(ReadError::Io(error)) => diagnostics.push(io_diagnostic(&error, source)),
            Err(ReadError::Unsafe) => {
                diagnostics.push(diagnostic(DiagnosticCode::UnsafePath, source))
            }
            Err(ReadError::Parse(error)) => diagnostics.push(diagnostic(
                match error {
                    ParseError::TooLarge | ParseError::Limit => DiagnosticCode::LimitExceeded,
                    _ => DiagnosticCode::InvalidFormat,
                },
                source,
            )),
        }
    }
    let mut candidates: Vec<_> = accounts
        .into_values()
        .map(|copies| {
            let latest = copies.iter().filter_map(|row| row.last_used_at).max();
            let names: BTreeSet<_> = copies
                .iter()
                .filter(|row| row.last_used_at == latest)
                .filter_map(|row| row.display_name.clone())
                .collect();
            Candidate {
                account_id: copies[0].account_id,
                steam64: copies[0].steam64.clone(),
                display_name: if names.len() == 1 {
                    names.into_iter().next()
                } else {
                    None
                },
                most_recent: copies.iter().any(|row| row.most_recent),
                last_used_at: latest,
            }
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.most_recent
            .cmp(&a.most_recent)
            .then(b.last_used_at.cmp(&a.last_used_at))
            .then(a.account_id.cmp(&b.account_id))
    });
    let errors = !diagnostics.is_empty();
    let recent: Vec<_> = candidates
        .iter()
        .filter(|row| row.most_recent)
        .map(|row| row.account_id)
        .collect();
    if recent.len() > 1 {
        diagnostics.push(diagnostic(
            DiagnosticCode::ConflictingMostRecent,
            "local_records",
        ));
    }
    let suggested_account_id = if errors || recent.len() > 1 {
        None
    } else if recent.len() == 1 {
        Some(recent[0])
    } else if candidates.len() == 1 {
        Some(candidates[0].account_id)
    } else {
        None
    };
    let status = if !candidates.is_empty() {
        if errors {
            DiscoveryStatus::Partial
        } else {
            DiscoveryStatus::Found
        }
    } else if diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::PermissionDenied)
    {
        DiscoveryStatus::PermissionDenied
    } else if diagnostics.iter().any(|d| {
        matches!(
            d.code,
            DiagnosticCode::InvalidFormat | DiagnosticCode::LimitExceeded
        )
    }) {
        DiscoveryStatus::InvalidFormat
    } else if !diagnostics.is_empty() {
        DiscoveryStatus::IoError
    } else if found_root {
        DiscoveryStatus::NoRecords
    } else {
        DiscoveryStatus::NotInstalled
    };
    DiscoveryReport {
        status,
        candidates,
        suggested_account_id,
        diagnostics,
        locations_checked,
    }
}

/// Explicit roots replace default discovery completely, including registry access.
/// Used by diagnostic CLIs and synthetic tests. No recursion or userdata scanning.
pub fn discover_in_roots(roots: &[PathBuf]) -> DiscoveryReport {
    assemble(
        roots
            .iter()
            .cloned()
            .map(|path| (path, "explicit_root"))
            .collect(),
        vec![],
    )
}

#[cfg(target_os = "linux")]
fn platform_roots() -> (Vec<(PathBuf, &'static str)>, Vec<Diagnostic>) {
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    else {
        return (
            vec![],
            vec![diagnostic(DiagnosticCode::NoHomeDirectory, "linux_home")],
        );
    };
    let mut roots: Vec<_> = [
        ".steam/steam",
        ".steam/root",
        ".steam/debian-installation",
        ".steam",
        ".local/share/Steam",
    ]
    .into_iter()
    .map(|path| (home.join(path), "linux_standard"))
    .collect();
    if let Some(data) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        roots.push((data.join("Steam"), "linux_xdg"));
    }
    for path in [
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".var/app/com.valvesoftware.Steam/data/Steam",
        ".var/app/com.valvesoftware.Steam/data/steam",
    ] {
        roots.push((home.join(path), "linux_flatpak"));
    }
    (roots, vec![])
}

#[cfg(windows)]
fn platform_roots() -> (Vec<(PathBuf, &'static str)>, Vec<Diagnostic>) {
    use winreg::{enums::KEY_READ, HKCU};
    let mut roots = vec![];
    let mut diagnostics = vec![];
    match HKCU.open_subkey_with_flags(r"Software\Valve\Steam", KEY_READ) {
        Ok(key) => match key.get_value::<String, _>("SteamPath") {
            Ok(path) if !path.trim().is_empty() => {
                roots.push((PathBuf::from(path), "windows_registry"))
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => diagnostics.push(io_diagnostic(&error, "windows_registry")),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => diagnostics.push(io_diagnostic(&error, "windows_registry")),
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = std::env::var_os(variable)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            roots.push((path.join("Steam"), "windows_default"));
        }
    }
    (roots, diagnostics)
}

/// Discover remembered accounts, not online sessions or authenticated ownership.
pub fn discover() -> DiscoveryReport {
    #[cfg(any(windows, target_os = "linux"))]
    {
        let (roots, diagnostics) = platform_roots();
        assemble(roots, diagnostics)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        DiscoveryReport {
            status: DiscoveryStatus::UnsupportedPlatform,
            candidates: vec![],
            suggested_account_id: None,
            diagnostics: vec![diagnostic(DiagnosticCode::UnsupportedPlatform, "platform")],
            locations_checked: 0,
        }
    }
}
