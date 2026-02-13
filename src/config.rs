use std::path::PathBuf;

pub struct Config {
    pub root_dirs: Vec<PathBuf>,
    pub index_path: PathBuf,
    pub max_file_size: u64,
    pub commit_interval: u64,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| home.join(".local/share"))
            .join("drozosearch")
            .join("index");

        Config {
            root_dirs: vec![home],
            index_path: data_dir,
            max_file_size: 10 * 1024 * 1024, // 10 MB
            commit_interval: 10_000,
        }
    }
}
