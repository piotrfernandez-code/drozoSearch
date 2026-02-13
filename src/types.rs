use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum MatchType {
    FileName,
    Content,
    Metadata,
}

impl std::fmt::Display for MatchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchType::FileName => write!(f, "Name"),
            MatchType::Content => write!(f, "Content"),
            MatchType::Metadata => write!(f, "Meta"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file_name: String,
    pub file_path: PathBuf,
    pub match_type: MatchType,
    pub file_size: u64,
    pub modified: i64,
    pub score: f32,
    pub content_snippet: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub files_indexed: u64,
    pub estimated_total: u64,
    pub status: IndexStatus,
}

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub added: u64,
    pub updated: u64,
    pub deleted: u64,
}

impl IndexStats {
    pub fn has_changes(&self) -> bool {
        self.added > 0 || self.updated > 0 || self.deleted > 0
    }
}

#[derive(Debug, Clone)]
pub enum IndexStatus {
    Counting,
    Starting,
    Indexing,
    Committing,
    Ready(Option<IndexStats>),
    Error(String),
}

impl std::fmt::Display for IndexStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexStatus::Counting => write!(f, "Scanning..."),
            IndexStatus::Starting => write!(f, "Starting..."),
            IndexStatus::Indexing => write!(f, "Indexing..."),
            IndexStatus::Committing => write!(f, "Committing..."),
            IndexStatus::Ready(_) => write!(f, "Ready"),
            IndexStatus::Error(e) => write!(f, "Error: {}", e),
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_time_ago(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - timestamp;

    if diff < 0 {
        return "just now".to_string();
    }

    let seconds = diff;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;

    if years > 0 {
        format!("{}y ago", years)
    } else if months > 0 {
        format!("{}mo ago", months)
    } else if weeks > 0 {
        format!("{}w ago", weeks)
    } else if days > 0 {
        format!("{}d ago", days)
    } else if hours > 0 {
        format!("{}h ago", hours)
    } else if minutes > 0 {
        format!("{}m ago", minutes)
    } else {
        "just now".to_string()
    }
}
