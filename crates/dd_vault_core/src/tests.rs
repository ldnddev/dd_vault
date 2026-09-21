use std::fs;
use std::path::Path;

use crate::{init, open, reindex, Error, Paths, Registry};

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    fs::write(path, body).expect("write");
}

#[test]
fn init_creates_layout_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("my-vault");
    let first = init(&root).expect("init");
    assert_eq!(first.name, "my-vault");
    assert!(first.meta_dir.ends_with(".dd_vault-my-vault"));
    assert!(root.join("notes/daily").is_dir());
    assert!(root.join("notes/projects").is_dir());
    assert!(root.join("assets").is_dir());
    assert!(root.join(".gitignore").is_file());
    assert!(first.meta_dir.join("config.toml").is_file());
    assert!(first.meta_dir.join("cache/preview").is_dir());
    assert!(first.meta_dir.join("logs").is_dir());
    assert!(first.meta_dir.join("sync").is_dir());
    let gitignore = fs::read_to_string(root.join(".gitignore")).expect("gi");
    assert!(gitignore.contains(".dd_vault-*/"));
    assert!(!first.meta_dir.join("index.db").exists());

    fs::write(root.join(".gitignore"), "keep-me\n").expect("custom gi");
    let second = init(&root).expect("re-init");
    assert_eq!(second.meta_dir, first.meta_dir);
    let gitignore = fs::read_to_string(root.join(".gitignore")).expect("gi2");
    assert_eq!(gitignore, "keep-me\n");
}

#[test]
fn open_requires_metadata_dir() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("plain");
    fs::create_dir_all(&root).expect("mkdir");
    let err = open(&root).expect_err("not a vault");
    assert!(matches!(err, Error::NotAVault(_)));
}

#[test]
fn open_after_init() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("notes");
    init(&root).expect("init");
    let vault = open(&root).expect("open");
    assert_eq!(vault.name, "notes");
    assert_eq!(vault.header_label(), " vault:notes ");
}

#[test]
fn open_missing_path() {
    let err = open(Path::new("/no/such/dd-vault-path-xyz")).expect_err("missing");
    assert!(matches!(err, Error::NotFound(_)));
}

#[test]
fn registry_roundtrip_and_last_path() {
    let dir = tempfile::tempdir().expect("tmp");
    let paths = Paths::new(dir.path());
    let vault_root = dir.path().join("work");
    let vault = init(&vault_root).expect("init");
    let mut registry = Registry::default();
    registry.register(&vault);
    registry.save(&paths).expect("save");
    let loaded = Registry::load(&paths).expect("load");
    assert_eq!(loaded.vaults.len(), 1);
    assert_eq!(loaded.vaults[0].name, "work");
    assert_eq!(loaded.last_path(), Some(vault.root.clone()));

    registry.register(&vault);
    assert_eq!(registry.vaults.len(), 1);

    assert!(registry.unregister(&vault.root.to_string_lossy()));
    assert!(registry.vaults.is_empty());
    assert!(registry.last_path().is_none());
}

#[test]
fn reindex_counts_markdown_and_skips_meta() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("vault");
    let vault = init(&root).expect("init");
    write(&root.join("notes/hello.md"), "# hi\n");
    write(&root.join("assets/skip.bin"), "xx");
    write(&vault.meta_dir.join("cache/preview/x.txt"), "nope");
    fs::create_dir_all(root.join(".git")).expect("git");
    write(&root.join(".git/config"), "nope");
    let report = reindex(&vault).expect("reindex");
    assert_eq!(report.markdown_files, 1);
    // assets/skip.bin; .gitignore, meta, and .git are skipped
    assert_eq!(report.other_files, 1);
}

#[test]
fn ambiguous_metadata_dirs_error() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("split");
    init(&root).expect("init");
    fs::create_dir_all(root.join(".dd_vault-other")).expect("second");
    let err = open(&root).expect_err("ambiguous");
    assert!(matches!(err, Error::Ambiguous(_)));
}

#[test]
fn walk_tree_skips_meta_and_sorts_dirs_first() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("vault");
    let vault = init(&root).expect("init");
    write(&root.join("notes/hello.md"), "# hi\n");
    write(&root.join("zeta.md"), "z\n");
    write(&vault.meta_dir.join("cache/preview/x.txt"), "nope");
    fs::create_dir_all(root.join(".git")).expect("git");
    let nodes = crate::walk_tree(&root).expect("walk");
    let names: Vec<_> = nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(names.contains(&"notes"));
    assert!(names.contains(&"assets"));
    assert!(names.contains(&"zeta.md"));
    assert!(!names.contains(&".gitignore"));
    assert!(!names.iter().any(|n| n.starts_with(".dd_vault")));
    assert!(!names.contains(&".git"));
    let first_files = names.iter().position(|n| *n == "zeta.md");
    let last_dir = names.iter().rposition(|n| *n == "notes" || *n == "assets");
    assert!(last_dir.unwrap() < first_files.unwrap());
    let notes = nodes.iter().find(|n| n.name == "notes").unwrap();
    assert!(notes.children.iter().any(|c| c.name == "hello.md"));
}

#[test]
fn create_rename_delete_file() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("vault");
    init(&root).expect("init");
    let created = crate::create_file(&root, Path::new("notes/idea.md")).expect("create");
    assert!(created.is_file());
    let renamed = crate::rename_entry(&root, Path::new("notes/idea.md"), "spark.md").expect("ren");
    assert!(renamed.ends_with("spark.md"));
    assert!(!root.join("notes/idea.md").exists());
    crate::delete_entry(&root, Path::new("notes/spark.md")).expect("del");
    assert!(!root.join("notes/spark.md").exists());
}

#[test]
fn reject_parent_dir_and_duplicate() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("vault");
    init(&root).expect("init");
    let err = crate::create_file(&root, Path::new("../escape.md")).expect_err("dotdot");
    assert!(matches!(err, Error::InvalidEntryName(_)));
    crate::create_file(&root, Path::new("notes/a.md")).expect("a");
    let err = crate::create_file(&root, Path::new("notes/a.md")).expect_err("dup");
    assert!(matches!(err, Error::AlreadyExists(_)));
}
