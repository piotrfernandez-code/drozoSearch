use std::path::Path;
use tantivy::{doc, Index, IndexWriter as TantivyWriter};

use super::schema::SchemaFields;
use crate::indexer::metadata::FileMetadata;

pub struct IndexWriter {
    writer: TantivyWriter,
    fields: SchemaFields,
    docs_since_commit: u64,
    commit_interval: u64,
}

impl IndexWriter {
    pub fn new(index: &Index, commit_interval: u64) -> tantivy::Result<Self> {
        let schema = index.schema();
        let fields = SchemaFields::new(&schema);
        // Use 50MB heap for the writer
        let writer = index.writer(50_000_000)?;
        Ok(IndexWriter {
            writer,
            fields,
            docs_since_commit: 0,
            commit_interval,
        })
    }

    pub fn add_file(
        &mut self,
        path: &Path,
        meta: &FileMetadata,
        content: Option<&str>,
    ) -> tantivy::Result<()> {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let file_path = path.to_string_lossy().to_string();
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut doc = doc!(
            self.fields.file_name => file_name,
            self.fields.file_path => file_path,
            self.fields.extension => extension,
            self.fields.file_size => meta.size,
            self.fields.modified => meta.modified,
            self.fields.created => meta.created,
            self.fields.permissions => meta.permissions.clone(),
            self.fields.is_dir => if meta.is_dir { 1u64 } else { 0u64 },
        );

        if let Some(text) = content {
            doc.add_text(self.fields.content, text);
        }

        self.writer.add_document(doc)?;
        self.docs_since_commit += 1;

        Ok(())
    }

    /// Returns true if a commit was performed
    pub fn maybe_commit(&mut self) -> tantivy::Result<bool> {
        if self.docs_since_commit >= self.commit_interval {
            self.commit()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn commit(&mut self) -> tantivy::Result<()> {
        self.writer.commit()?;
        self.docs_since_commit = 0;
        Ok(())
    }

    /// Delete all documents matching a term (used for incremental re-indexing)
    pub fn delete_term(&mut self, term: tantivy::Term) {
        self.writer.delete_term(term);
    }
}
