use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::thread;

use tantivy::schema::Value;

use crate::config::Config;
use crate::index::schema::SchemaFields;
use crate::index::writer::IndexWriter;
use crate::indexer::content;
use crate::indexer::metadata::FileMetadata;
use crate::indexer::walker;
use crate::types::{IndexProgress, IndexStats, IndexStatus};

pub fn start_indexing(
    index: tantivy::Index,
    config: Config,
    progress_tx: Sender<IndexProgress>,
    ctx: eframe::egui::Context,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        run_indexing(index, config, progress_tx, ctx);
    })
}

/// Load existing indexed files as a map of (path → modified_timestamp).
fn load_existing_index(index: &tantivy::Index) -> HashMap<String, i64> {
    let mut existing = HashMap::new();
    let reader = match index.reader() {
        Ok(r) => r,
        Err(_) => return existing,
    };
    let searcher = reader.searcher();
    let schema = index.schema();
    let fields = SchemaFields::new(&schema);

    for segment_reader in searcher.segment_readers() {
        let store = segment_reader.get_store_reader(64).ok();
        let store = match store {
            Some(s) => s,
            None => continue,
        };
        for doc_id in 0..segment_reader.num_docs() {
            if let Ok(doc) = store.get::<tantivy::TantivyDocument>(doc_id) {
                let path = doc
                    .get_first(fields.file_path)
                    .and_then(|v: &tantivy::schema::OwnedValue| v.as_str())
                    .map(|s: &str| s.to_string());
                let modified = doc
                    .get_first(fields.modified)
                    .and_then(|v: &tantivy::schema::OwnedValue| v.as_i64());
                if let (Some(p), Some(m)) = (path, modified) {
                    existing.insert(p, m);
                }
            }
        }
    }
    existing
}

fn run_indexing(
    index: tantivy::Index,
    config: Config,
    progress_tx: Sender<IndexProgress>,
    ctx: eframe::egui::Context,
) {
    // ── Load existing index state ──
    let _ = progress_tx.send(IndexProgress {
        files_indexed: 0,
        estimated_total: 0,
        status: IndexStatus::Counting,
    });
    ctx.request_repaint();

    let mut existing = load_existing_index(&index);
    let had_existing = !existing.is_empty();
    let existing_count = existing.len() as u64;

    // If index already has data, show it as ready immediately so search works
    // while we do an incremental update in the background
    if had_existing {
        let _ = progress_tx.send(IndexProgress {
            files_indexed: existing_count,
            estimated_total: existing_count,
            status: IndexStatus::Ready(None),
        });
        ctx.request_repaint();
    }

    // ── Phase 1: Quick file count scan ──
    let estimated_total = quick_count(&config.root_dirs, &progress_tx, &ctx, had_existing);

    let mut writer = match IndexWriter::new(&index, config.commit_interval) {
        Ok(w) => w,
        Err(e) => {
            let _ = progress_tx.send(IndexProgress {
                files_indexed: existing_count,
                estimated_total: existing_count,
                status: IndexStatus::Error(e.to_string()),
            });
            ctx.request_repaint();
            return;
        }
    };

    // Create a channel for the walker to send paths
    let (path_tx, path_rx) = std::sync::mpsc::channel();

    let roots = config.root_dirs.clone();
    let walker_handle = thread::spawn(move || {
        walker::walk_paths(&roots, path_tx);
    });

    let mut files_scanned: u64 = 0;
    let mut files_added: u64 = 0;
    let mut files_updated: u64 = 0;
    let mut need_commit = false;

    for path in path_rx {
        files_scanned += 1;

        let path_str = path.to_string_lossy().to_string();

        // Check if this file is already indexed with the same modified time
        let meta = match FileMetadata::from_path(&path) {
            Some(m) => m,
            None => {
                existing.remove(&path_str);
                continue;
            }
        };

        if let Some(&indexed_modified) = existing.get(&path_str) {
            if indexed_modified == meta.modified {
                // File unchanged — skip it
                existing.remove(&path_str);

                // Still send progress updates during scan
                if files_scanned % 2000 == 0 {
                    let _ = progress_tx.send(IndexProgress {
                        files_indexed: existing_count + files_added,
                        estimated_total: estimated_total.max(existing_count + files_added),
                        status: IndexStatus::Indexing,
                    });
                    ctx.request_repaint();
                }
                continue;
            }
            // File modified — delete old version, will re-add below
            let schema = index.schema();
            let fields = SchemaFields::new(&schema);
            let term = tantivy::Term::from_field_text(fields.file_path, &path_str);
            writer.delete_term(term);
            existing.remove(&path_str);
            files_updated += 1;
        } else {
            files_added += 1;
        }

        let file_content = if !meta.is_dir {
            content::read_content(&path, config.max_file_size)
        } else {
            None
        };

        if writer
            .add_file(&path, &meta, file_content.as_deref())
            .is_err()
        {
            continue;
        }

        need_commit = true;

        // Periodic commit and progress update
        if let Ok(true) = writer.maybe_commit() {
            let _ = progress_tx.send(IndexProgress {
                files_indexed: existing_count + files_added,
                estimated_total: estimated_total.max(existing_count + files_added),
                status: IndexStatus::Indexing,
            });
            ctx.request_repaint();
        }

        if (files_added + files_updated) % 500 == 0 {
            let _ = progress_tx.send(IndexProgress {
                files_indexed: existing_count + files_added,
                estimated_total: estimated_total.max(existing_count + files_added),
                status: IndexStatus::Indexing,
            });
            ctx.request_repaint();
        }
    }

    let _ = walker_handle.join();

    // ── Delete files that no longer exist on disk ──
    if !existing.is_empty() {
        let schema = index.schema();
        let fields = SchemaFields::new(&schema);
        for path_str in existing.keys() {
            let term = tantivy::Term::from_field_text(fields.file_path, path_str);
            writer.delete_term(term);
            need_commit = true;
        }
    }

    let deleted = existing.len() as u64;
    let total_indexed = existing_count + files_added - deleted;

    // Only commit if something actually changed
    if need_commit {
        let _ = progress_tx.send(IndexProgress {
            files_indexed: total_indexed,
            estimated_total: total_indexed,
            status: IndexStatus::Committing,
        });
        ctx.request_repaint();

        if let Err(e) = writer.commit() {
            let _ = progress_tx.send(IndexProgress {
                files_indexed: total_indexed,
                estimated_total: total_indexed,
                status: IndexStatus::Error(e.to_string()),
            });
            ctx.request_repaint();
            return;
        }
    }

    let stats = IndexStats {
        added: files_added,
        updated: files_updated,
        deleted,
    };
    let _ = progress_tx.send(IndexProgress {
        files_indexed: total_indexed,
        estimated_total: total_indexed,
        status: IndexStatus::Ready(if stats.has_changes() { Some(stats) } else { None }),
    });
    ctx.request_repaint();
}

/// Fast pre-scan: count files without reading metadata or content.
/// Sends counting progress updates so the UI stays responsive.
/// When `quiet` is true (incremental update), don't overwrite the Ready status.
fn quick_count(
    roots: &[std::path::PathBuf],
    progress_tx: &Sender<IndexProgress>,
    ctx: &eframe::egui::Context,
    quiet: bool,
) -> u64 {
    use ignore::WalkBuilder;

    let mut count: u64 = 0;

    for root in roots {
        let walker = WalkBuilder::new(root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .follow_links(false)
            .max_depth(Some(20))
            .filter_entry(|entry| {
                if entry.file_type().map_or(false, |ft| ft.is_dir()) {
                    if let Some(name) = entry.file_name().to_str() {
                        let skip = [
                            ".git", "node_modules", "target", ".cache", ".Trash",
                            "__pycache__", ".tox", ".venv", "venv", ".env", "dist",
                            "build", ".build", ".gradle", ".idea", ".vscode",
                            "Library", ".Spotlight-V100", ".fseventsd",
                        ];
                        if skip.contains(&name) {
                            return false;
                        }
                    }
                }
                true
            })
            .build();

        for entry in walker {
            if entry.is_ok() {
                count += 1;
                // Update UI every 5000 files during counting (only for fresh index)
                if !quiet && count % 5000 == 0 {
                    let _ = progress_tx.send(IndexProgress {
                        files_indexed: 0,
                        estimated_total: count,
                        status: IndexStatus::Counting,
                    });
                    ctx.request_repaint();
                }
            }
        }
    }

    count
}
