use tantivy::schema::*;

pub fn build_schema() -> Schema {
    let mut builder = Schema::builder();

    // File name - tokenized for partial matching, stored for display
    builder.add_text_field("file_name", TEXT | STORED);

    // Full file path - stored for display, indexed as raw string
    let path_options = TextOptions::default()
        .set_stored()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("raw")
                .set_index_option(IndexRecordOption::Basic),
        );
    builder.add_text_field("file_path", path_options);

    // File extension - indexed as single token for filtering
    builder.add_text_field("extension", STRING | STORED);

    // File content - tokenized full-text, NOT stored to save disk space
    builder.add_text_field("content", TEXT);

    // File size in bytes
    builder.add_u64_field("file_size", INDEXED | STORED | FAST);

    // Last modified timestamp as unix seconds
    builder.add_i64_field("modified", INDEXED | STORED | FAST);

    // Created timestamp
    builder.add_i64_field("created", STORED | FAST);

    // Permissions string (e.g. "rwxr-xr-x")
    builder.add_text_field("permissions", STRING | STORED);

    // Is directory flag
    builder.add_u64_field("is_dir", INDEXED | STORED);

    builder.build()
}

/// Helper to get all field handles from a schema
pub struct SchemaFields {
    pub file_name: Field,
    pub file_path: Field,
    pub extension: Field,
    pub content: Field,
    pub file_size: Field,
    pub modified: Field,
    pub created: Field,
    pub permissions: Field,
    pub is_dir: Field,
}

impl SchemaFields {
    pub fn new(schema: &Schema) -> Self {
        SchemaFields {
            file_name: schema.get_field("file_name").unwrap(),
            file_path: schema.get_field("file_path").unwrap(),
            extension: schema.get_field("extension").unwrap(),
            content: schema.get_field("content").unwrap(),
            file_size: schema.get_field("file_size").unwrap(),
            modified: schema.get_field("modified").unwrap(),
            created: schema.get_field("created").unwrap(),
            permissions: schema.get_field("permissions").unwrap(),
            is_dir: schema.get_field("is_dir").unwrap(),
        }
    }
}
