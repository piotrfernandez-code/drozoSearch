use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub modified: i64,
    pub created: i64,
    pub permissions: String,
    pub is_dir: bool,
}

impl FileMetadata {
    pub fn from_path(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;

        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let created = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let permissions = format_permissions(&meta);

        Some(FileMetadata {
            size: meta.len(),
            modified,
            created,
            permissions,
            is_dir: meta.is_dir(),
        })
    }
}

#[cfg(unix)]
fn format_permissions(meta: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let mut s = String::with_capacity(9);
    let flags = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    for (bit, ch) in flags {
        s.push(if mode & bit != 0 { ch } else { '-' });
    }
    s
}

#[cfg(not(unix))]
fn format_permissions(meta: &fs::Metadata) -> String {
    if meta.permissions().readonly() {
        "readonly".to_string()
    } else {
        "readwrite".to_string()
    }
}
