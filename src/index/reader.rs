use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::Value;
use tantivy::{Index, ReloadPolicy};

use super::schema::SchemaFields;
use crate::types::{MatchType, SearchResult};

pub struct SearchEngine {
    index: Index,
    fields: SchemaFields,
}

impl SearchEngine {
    pub fn new(index: Index) -> Self {
        let fields = SchemaFields::new(&index.schema());
        SearchEngine { index, fields }
    }

    pub fn search(&self, query_str: &str, limit: usize) -> Vec<SearchResult> {
        if query_str.trim().is_empty() {
            return vec![];
        }

        let reader = match self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
        {
            Ok(r) => r,
            Err(_) => return vec![],
        };

        let searcher = reader.searcher();

        let mut query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.file_name,
                self.fields.content,
                self.fields.extension,
            ],
        );
        query_parser.set_field_boost(self.fields.file_name, 3.0);
        query_parser.set_field_boost(self.fields.extension, 1.5);

        let query = match query_parser.parse_query(query_str) {
            Ok(q) => q,
            Err(_) => {
                let escaped: String = query_str
                    .chars()
                    .map(|c| {
                        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
                            format!("\\{}", c)
                        } else {
                            c.to_string()
                        }
                    })
                    .collect();
                match query_parser.parse_query(&escaped) {
                    Ok(q) => q,
                    Err(_) => return vec![],
                }
            }
        };

        // Retrieve more candidates than needed — we'll re-rank and trim
        let retrieve_limit = (limit * 3).min(600);
        let top_docs = match searcher.search(&query, &TopDocs::with_limit(retrieve_limit)) {
            Ok(docs) => docs,
            Err(_) => return vec![],
        };

        let query_lower = query_str.to_lowercase();
        let now_ts = chrono::Utc::now().timestamp();

        let mut results: Vec<SearchResult> = top_docs
            .into_iter()
            .filter_map(|(bm25_score, doc_address)| {
                let doc: tantivy::TantivyDocument = searcher.doc(doc_address).ok()?;

                let file_name = doc
                    .get_first(self.fields.file_name)?
                    .as_str()?
                    .to_string();
                let file_path_str = doc
                    .get_first(self.fields.file_path)?
                    .as_str()?
                    .to_string();
                let file_size = doc.get_first(self.fields.file_size)?.as_u64()?;
                let modified = doc.get_first(self.fields.modified)?.as_i64()?;
                let is_dir_val = doc.get_first(self.fields.is_dir)?.as_u64()?;
                let is_dir = is_dir_val == 1;

                let file_name_lower = file_name.to_lowercase();
                let path = PathBuf::from(&file_path_str);

                // ── Determine match type ──
                let match_type = if file_name_lower.contains(&query_lower) {
                    MatchType::FileName
                } else {
                    MatchType::Content
                };

                // ── Compute composite score ──
                let final_score =
                    compute_rank(bm25_score, &query_lower, &file_name_lower, &path, modified, is_dir, now_ts);

                Some(SearchResult {
                    file_name,
                    file_path: path,
                    match_type,
                    file_size,
                    modified,
                    score: final_score,
                    content_snippet: None,
                    is_dir,
                })
            })
            .collect();

        // Sort by our composite score (highest first)
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }
}

/// Composite ranking function.
///
/// Blends multiple signals into a single score:
///   1. BM25 (normalized)     — baseline text relevance
///   2. Exact name match      — huge bonus if file name == query
///   3. Name starts-with      — bonus if file name starts with query
///   4. Name contains         — moderate bonus for substring match in name
///   5. Recency               — recently modified files score higher
///   6. Path depth penalty    — deeply nested files score lower
///   7. File > directory      — files are usually more relevant
///
/// All signals are combined as weighted sum. Weights were tuned by hand
/// to produce intuitive results for common search patterns.
fn compute_rank(
    bm25: f32,
    query_lower: &str,
    file_name_lower: &str,
    path: &std::path::Path,
    modified_ts: i64,
    is_dir: bool,
    now_ts: i64,
) -> f32 {
    // ── 1. Normalize BM25 to roughly 0..1 range ──
    // BM25 scores typically range 0..30 depending on corpus. Sigmoid squash.
    let bm25_norm = bm25 / (bm25 + 10.0);

    // ── 2. Exact name match (massive bonus) ──
    // "main.rs" searching "main.rs" → top result
    let exact_bonus = if file_name_lower == query_lower {
        1.0
    } else {
        // Also check without extension: searching "main" matches "main.rs"
        let stem = file_name_lower
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(file_name_lower);
        if stem == query_lower {
            0.8
        } else {
            0.0
        }
    };

    // ── 3. Name starts-with bonus ──
    // Searching "read" → README.md beats thread_pool.rs
    let starts_with_bonus = if exact_bonus == 0.0 && file_name_lower.starts_with(query_lower) {
        0.5
    } else {
        0.0
    };

    // ── 4. Name contains bonus ──
    // Any substring match in file name is better than content-only match
    let contains_bonus = if exact_bonus == 0.0
        && starts_with_bonus == 0.0
        && file_name_lower.contains(query_lower)
    {
        0.3
    } else {
        0.0
    };

    // ── 5. Recency signal ──
    // Log-decay: files modified recently score higher.
    // 1 hour ago → ~1.0, 1 day → ~0.75, 1 week → ~0.6, 1 year → ~0.35, 5 years → ~0.25
    let age_seconds = (now_ts - modified_ts).max(1) as f64;
    let age_hours = age_seconds / 3600.0;
    let recency = 1.0 / (1.0 + (age_hours / 24.0).ln().max(0.0)) as f32;

    // ── 6. Path depth penalty ──
    // Fewer components = more likely to be a "main" file.
    // ~/project/src/main.rs (4 components) scores higher than
    // ~/backup/old/archive/2019/project/src/main.rs (8 components)
    let depth = path.components().count() as f32;
    let depth_penalty = 1.0 / (1.0 + (depth - 3.0).max(0.0) * 0.08);

    // ── 7. File vs directory ──
    let type_bonus: f32 = if is_dir { 0.0 } else { 0.1 };

    // ── Weighted combination ──
    let score = bm25_norm * 2.0        // baseline relevance
        + exact_bonus * 5.0            // exact match dominates
        + starts_with_bonus * 2.0      // prefix match is strong
        + contains_bonus * 1.5         // substring in name is good
        + recency * 0.8               // recent files get a bump
        + depth_penalty * 0.4         // shallow paths preferred
        + type_bonus;                  // files over directories

    score
}
