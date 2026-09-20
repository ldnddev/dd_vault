use std::fs;
use std::path::{Path, PathBuf};

use crate::skip::skip_entry_name;
use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Dir,
    File,
    Symlink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsNode {
    pub name: String,
    pub rel: PathBuf,
    pub kind: NodeKind,
    pub children: Vec<FsNode>,
}

impl FsNode {
    pub fn is_dir(&self) -> bool {
        self.kind == NodeKind::Dir
    }
}

/// Recursively list vault contents, skipping metadata / git / db junk.
pub fn walk_tree(root: &Path) -> Result<Vec<FsNode>, Error> {
    walk_dir(root, PathBuf::new())
}

fn walk_dir(root: &Path, rel: PathBuf) -> Result<Vec<FsNode>, Error> {
    let abs = root.join(&rel);
    let mut nodes = Vec::new();
    let entries = match fs::read_dir(&abs) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(nodes),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy().into_owned();
        let child_rel = if rel.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            rel.join(&name)
        };
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let ft = meta.file_type();
        let is_dir = ft.is_dir() && !ft.is_symlink();
        if skip_entry_name(&name, is_dir) {
            continue;
        }
        let kind = if ft.is_symlink() {
            NodeKind::Symlink
        } else if is_dir {
            NodeKind::Dir
        } else {
            NodeKind::File
        };
        let children = if kind == NodeKind::Dir {
            walk_dir(root, child_rel.clone())?
        } else {
            Vec::new()
        };
        nodes.push(FsNode {
            name,
            rel: child_rel,
            kind,
            children,
        });
    }
    nodes.sort_by(|a, b| {
        let dir_a = a.kind == NodeKind::Dir;
        let dir_b = b.kind == NodeKind::Dir;
        dir_b
            .cmp(&dir_a)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(nodes)
}

impl FsNode {
    pub fn matches_filter(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let q = query.to_lowercase();
        if self.name.to_lowercase().contains(&q) {
            return true;
        }
        self.children.iter().any(|c| c.matches_filter(query))
    }
}
