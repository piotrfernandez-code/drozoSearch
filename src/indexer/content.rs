use std::fs;
use std::io::Read;
use std::path::Path;

/// Known text file extensions that we should index content for
const TEXT_EXTENSIONS: &[&str] = &[
    // Programming
    "rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "h", "cpp", "hpp",
    "java", "rb", "php", "swift", "kt", "scala", "r", "m", "mm",
    "cs", "fs", "vb", "lua", "pl", "pm", "hs", "erl", "ex", "exs",
    "clj", "cljs", "dart", "zig", "nim", "v", "d", "ada", "adb",
    // Shell & config
    "sh", "bash", "zsh", "fish", "ps1", "bat", "cmd",
    "toml", "yaml", "yml", "json", "xml", "ini", "cfg", "conf",
    "env", "properties", "gradle",
    // Web
    "html", "htm", "css", "scss", "sass", "less", "vue", "svelte",
    // Documents
    "md", "markdown", "txt", "rst", "tex", "org", "adoc",
    // Data
    "csv", "tsv", "sql", "graphql", "gql",
    // Other
    "log", "diff", "patch", "gitignore", "dockerignore",
    "dockerfile", "makefile", "cmake", "meson",
];

/// Check if a file should have its content indexed
pub fn is_text_file(path: &Path) -> bool {
    // Check extension first (fast path)
    if let Some(ext) = path.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        if TEXT_EXTENSIONS.contains(&ext_lower.as_str()) {
            return true;
        }
    }

    // Check for extensionless known files
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy().to_lowercase();
        if matches!(
            name.as_str(),
            "makefile" | "dockerfile" | "gemfile" | "rakefile" | "procfile"
                | "vagrantfile" | "justfile" | "cmakelists.txt"
        ) {
            return true;
        }
    }

    false
}

/// Check if file content appears to be binary (has null bytes in first 8KB)
fn is_binary_content(path: &Path) -> bool {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return true, // treat errors as binary
    };

    let mut buf = [0u8; 8192];
    let bytes_read = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return true,
    };

    buf[..bytes_read].contains(&0)
}

/// Read file content for indexing, with size limit
pub fn read_content(path: &Path, max_size: u64) -> Option<String> {
    // Check size first
    let meta = fs::metadata(path).ok()?;
    if meta.len() > max_size || meta.len() == 0 {
        return None;
    }

    if !is_text_file(path) {
        return None;
    }

    if is_binary_content(path) {
        return None;
    }

    fs::read_to_string(path).ok()
}
