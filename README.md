# drozoSearch

Lightning-fast cross-platform desktop search. Indexes file names, content, and metadata across your entire home directory and lets you find anything instantly.

Built in Rust with a native GUI.

## Features

- **Full-text search** - searches file names, file content, and metadata in a single query
- **Incremental indexing** - first run builds a full index, subsequent launches only process new/modified/deleted files
- **System tray** - lives in your menu bar, close the window and it keeps running
- **Click to open** - single click opens a file with its default app, Shift+click lets you choose which app
- **Keyboard navigation** - arrow keys, Enter to open, Escape to clear
- **Search filters** - use `name:`, `ext:`, `size>1mb` to narrow results
- **Dark theme** with file type icons, match type badges (NAME / CONTENT / META), and a real-time progress bar during indexing

## How it works

drozoSearch uses three threads:

| Thread | Role |
|--------|------|
| **GUI** (main) | eframe/egui render loop, never blocks |
| **Search** | Reads queries from a channel, searches the tantivy index, sends results back |
| **Indexer** | Walks the filesystem with the `ignore` crate, reads text file content, writes to tantivy |

The index is stored on disk at:
- **macOS**: `~/Library/Application Support/drozosearch/index/`
- **Linux**: `~/.local/share/drozosearch/index/`

Text files up to 10 MB are content-indexed. File content is not stored in the index (only indexed for search), keeping disk usage low.

## Tech stack

- [eframe](https://github.com/emilk/egui/tree/master/crates/eframe) / [egui](https://github.com/emilk/egui) - native GUI
- [tantivy](https://github.com/quickwit-oss/tantivy) - full-text search engine (Rust's Lucene)
- [ignore](https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore) - parallel filesystem walk (from ripgrep), respects `.gitignore`
- [tray-icon](https://github.com/nicbarker/tray-icon) - system tray integration
- [open](https://github.com/Byron/open-rs) - open files with default app

## Building from source

Requires [Rust](https://rustup.rs/) (stable).

```bash
# Run in debug mode
cargo run

# Build optimized release binary
cargo build --release

# macOS: create .app bundle
bash bundle-macos.sh
```

### Linux dependencies

```bash
sudo apt-get install -y \
  libgtk-3-dev \
  libxdo-dev \
  libayatana-appindicator3-dev \
  libgl1-mesa-dev \
  libx11-dev \
  libxrandr-dev \
  libxi-dev
```

## Installing on macOS

After building, either run the bundle script or copy the app manually:

```bash
bash bundle-macos.sh
cp -r target/drozoSearch.app /Applications/
```

## Releases

Pre-built binaries for macOS (arm64 + x64), Linux (x64), and Windows (x64) are published automatically via GitHub Actions when a version tag is pushed:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## License

MIT
