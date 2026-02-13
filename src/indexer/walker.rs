use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use ignore::WalkBuilder;

/// Directories to always skip
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".cache",
    ".Trash",
    "__pycache__",
    ".tox",
    ".venv",
    "venv",
    ".env",
    "dist",
    "build",
    ".build",
    ".gradle",
    ".idea",
    ".vscode",
    "Library",
    ".Spotlight-V100",
    ".fseventsd",
];

/// Walk the filesystem from the given roots, sending discovered paths to the channel
pub fn walk_paths(roots: &[PathBuf], tx: Sender<PathBuf>) {
    for root in roots {
        walk_single_root(root, &tx);
    }
}

fn walk_single_root(root: &Path, tx: &Sender<PathBuf>) {
    let walker = WalkBuilder::new(root)
        .hidden(false) // include hidden files
        .git_ignore(true) // respect .gitignore
        .git_global(true)
        .git_exclude(true)
        .follow_links(false) // avoid symlink loops
        .max_depth(Some(20)) // don't go too deep
        .filter_entry(|entry| {
            // Skip known heavy directories
            if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                if let Some(name) = entry.file_name().to_str() {
                    if SKIP_DIRS.contains(&name) {
                        return false;
                    }
                }
            }
            true
        })
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue, // skip permission errors etc
        };

        let path = entry.into_path();
        if tx.send(path).is_err() {
            return; // receiver dropped, stop walking
        }
    }
}
