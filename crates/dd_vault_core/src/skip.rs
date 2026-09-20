use crate::paths::is_metadata_dirname;

pub fn skip_dir_name(name: &str) -> bool {
    name == ".git" || is_metadata_dirname(name)
}

pub fn skip_file_name(name: &str) -> bool {
    name == ".DS_Store"
        || name == ".gitignore"
        || name.ends_with(".db")
        || name.ends_with(".db-wal")
        || name.ends_with(".db-shm")
}

pub fn skip_entry_name(name: &str, is_dir: bool) -> bool {
    if is_dir {
        skip_dir_name(name)
    } else {
        skip_file_name(name) || skip_dir_name(name)
    }
}
