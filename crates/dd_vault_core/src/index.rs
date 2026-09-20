//! SQLite derived cache + FTS5.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::parse::{parse_note, LinkKind, ParsedNote};
use crate::skip::skip_entry_name;
use crate::{Error, Vault};

const SCHEMA_VERSION: i64 = 1;
const FTS_MAX_BYTES: u64 = 5 * 1024 * 1024;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  mtime INTEGER NOT NULL,
  size INTEGER NOT NULL,
  hash TEXT NOT NULL,
  kind TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS notes (
  file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  frontmatter_json TEXT NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS aliases (
  note_id INTEGER NOT NULL REFERENCES notes(file_id) ON DELETE CASCADE,
  alias TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tags (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  parent_id INTEGER REFERENCES tags(id)
);
CREATE TABLE IF NOT EXISTS note_tags (
  note_id INTEGER NOT NULL REFERENCES notes(file_id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (note_id, tag_id)
);
CREATE TABLE IF NOT EXISTS links (
  id INTEGER PRIMARY KEY,
  src_note_id INTEGER NOT NULL REFERENCES notes(file_id) ON DELETE CASCADE,
  dst_raw TEXT NOT NULL,
  dst_note_id INTEGER REFERENCES notes(file_id),
  dst_heading TEXT,
  kind TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS headings (
  id INTEGER PRIMARY KEY,
  note_id INTEGER NOT NULL REFERENCES notes(file_id) ON DELETE CASCADE,
  level INTEGER NOT NULL,
  text TEXT NOT NULL,
  line INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS fts_notes USING fts5(
  title, aliases, body, note_id UNINDEXED
);
"#;

#[derive(Debug)]
pub struct Index {
    conn: Connection,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHit {
    pub path: String,
    pub title: String,
    pub snippet: Option<String>,
    pub heading: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReindexReport {
    pub markdown_files: usize,
    pub other_files: usize,
    pub notes_indexed: usize,
}

impl Index {
    pub fn open(meta_dir: &Path) -> Result<Self, Error> {
        fs::create_dir_all(meta_dir)?;
        let path = meta_dir.join("index.db");
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        let version: Option<i64> = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        if version.unwrap_or(0) < SCHEMA_VERSION {
            conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )?;
        }
        Ok(Self { conn, path })
    }

    pub fn rebuild(vault: &Vault) -> Result<ReindexReport, Error> {
        crate::vault::ensure_layout(&vault.root, &vault.meta_dir)?;
        let idx = Self::open(&vault.meta_dir)?;
        idx.conn.execute_batch(
            "DELETE FROM fts_notes;
             DELETE FROM headings;
             DELETE FROM links;
             DELETE FROM note_tags;
             DELETE FROM aliases;
             DELETE FROM notes;
             DELETE FROM files;",
        )?;
        let mut report = ReindexReport {
            markdown_files: 0,
            other_files: 0,
            notes_indexed: 0,
        };
        let mut files = Vec::new();
        collect_files(&vault.root, &vault.root, &mut files)?;
        for f in &files {
            let kind = file_kind(&f.rel);
            if kind == "note" {
                report.markdown_files += 1;
            } else {
                report.other_files += 1;
            }
            idx.conn.execute(
                "INSERT INTO files (path, mtime, size, hash, kind) VALUES (?1,?2,?3,?4,?5)",
                params![
                    f.rel.to_string_lossy(),
                    f.mtime as i64,
                    f.size as i64,
                    f.hash,
                    kind
                ],
            )?;
            let file_id = idx.conn.last_insert_rowid();
            if kind == "note" && f.size <= FTS_MAX_BYTES {
                if let Ok(src) = fs::read_to_string(&f.abs) {
                    let parsed = parse_note(&f.rel, &src);
                    idx.insert_note(file_id, &parsed)?;
                    report.notes_indexed += 1;
                }
            }
        }
        idx.resolve_links()?;
        Ok(report)
    }

    fn insert_note(&self, file_id: i64, parsed: &ParsedNote) -> Result<(), Error> {
        self.conn.execute(
            "INSERT INTO notes (file_id, title, frontmatter_json) VALUES (?1,?2,?3)",
            params![file_id, parsed.title, parsed.frontmatter_json],
        )?;
        for alias in &parsed.aliases {
            self.conn.execute(
                "INSERT INTO aliases (note_id, alias) VALUES (?1,?2)",
                params![file_id, alias],
            )?;
        }
        for tag in &parsed.tags {
            self.ensure_tag(tag)?;
            let tag_id: i64 =
                self.conn
                    .query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| {
                        r.get(0)
                    })?;
            self.conn.execute(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1,?2)",
                params![file_id, tag_id],
            )?;
        }
        for h in &parsed.headings {
            self.conn.execute(
                "INSERT INTO headings (note_id, level, text, line) VALUES (?1,?2,?3,?4)",
                params![file_id, h.level, h.text, h.line],
            )?;
        }
        for l in &parsed.links {
            let kind = match l.kind {
                LinkKind::Wiki => "wiki",
                LinkKind::Md => "md",
                LinkKind::Embed => "embed",
            };
            self.conn.execute(
                "INSERT INTO links (src_note_id, dst_raw, dst_heading, kind) VALUES (?1,?2,?3,?4)",
                params![file_id, l.dst_raw, l.dst_heading, kind],
            )?;
        }
        let aliases = parsed.aliases.join(" ");
        self.conn.execute(
            "INSERT INTO fts_notes (title, aliases, body, note_id) VALUES (?1,?2,?3,?4)",
            params![parsed.title, aliases, parsed.body, file_id],
        )?;
        Ok(())
    }

    fn ensure_tag(&self, name: &str) -> Result<(), Error> {
        let parent = name.rsplit_once('/').map(|(p, _)| p);
        let parent_id = if let Some(p) = parent {
            self.ensure_tag(p)?;
            Some(
                self.conn
                    .query_row("SELECT id FROM tags WHERE name = ?1", params![p], |r| {
                        r.get::<_, i64>(0)
                    })?,
            )
        } else {
            None
        };
        self.conn.execute(
            "INSERT OR IGNORE INTO tags (name, parent_id) VALUES (?1,?2)",
            params![name, parent_id],
        )?;
        Ok(())
    }

    fn resolve_links(&self) -> Result<(), Error> {
        let rows: Vec<(i64, String)> = {
            let mut stmt = self.conn.prepare("SELECT id, dst_raw FROM links")?;
            let collected = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            collected
        };
        for (id, raw) in rows {
            if let Some(note_id) = self.lookup_note(&raw)? {
                self.conn.execute(
                    "UPDATE links SET dst_note_id = ?1 WHERE id = ?2",
                    params![note_id, id],
                )?;
            }
        }
        Ok(())
    }

    fn lookup_note(&self, raw: &str) -> Result<Option<i64>, Error> {
        let stem = Path::new(raw)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(raw);
        let id = self
            .conn
            .query_row(
                "SELECT n.file_id FROM notes n
                 JOIN files f ON f.id = n.file_id
                 WHERE n.title = ?1
                    OR f.path = ?2
                    OR f.path LIKE ?3
                    OR EXISTS (SELECT 1 FROM aliases a WHERE a.note_id = n.file_id AND a.alias = ?1)
                 LIMIT 1",
                params![
                    stem,
                    raw,
                    format!(
                        "%/{}",
                        if raw.ends_with(".md") {
                            raw.to_string()
                        } else {
                            format!("{stem}.md")
                        }
                    )
                ],
                |r| r.get(0),
            )
            .optional()?;
        Ok(id)
    }

    pub fn search_files(&self, query: &str) -> Result<Vec<FileHit>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path,
                    COALESCE(n.title, f.path),
                    COALESCE((SELECT GROUP_CONCAT(a.alias, ' ') FROM aliases a WHERE a.note_id = n.file_id), '')
             FROM files f
             LEFT JOIN notes n ON n.file_id = f.id
             ORDER BY f.path",
        )?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(fuzzy_hits(&rows, query, 50))
    }

    pub fn search_content(&self, query: &str) -> Result<Vec<FileHit>, Error> {
        let Some(match_q) = fts_query(query) else {
            return self.search_files(query);
        };
        // FTS5 treats bound parameters as phrases, so prefix `*` must be in the SQL.
        let sql = format!(
            "SELECT f.path, n.title, snippet(fts_notes, 2, '', '', '…', 10)
             FROM fts_notes
             JOIN notes n ON n.file_id = fts_notes.note_id
             JOIN files f ON f.id = n.file_id
             WHERE fts_notes MATCH '{match_q}'
             LIMIT 80"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return self.search_files(query);
        }
        Ok(rows
            .into_iter()
            .take(50)
            .map(|(path, title, snippet)| FileHit {
                path,
                title,
                snippet: Some(snippet),
                heading: None,
            })
            .collect())
    }

    pub fn search_tags(&self, query: &str) -> Result<Vec<FileHit>, Error> {
        let mut stmt = self.conn.prepare(
            "SELECT f.path, n.title, t.name FROM note_tags nt
             JOIN tags t ON t.id = nt.tag_id
             JOIN notes n ON n.file_id = nt.note_id
             JOIN files f ON f.id = n.file_id
             ORDER BY t.name, f.path",
        )?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let pairs: Vec<(String, String, String)> = rows
            .iter()
            .map(|(p, title, tag)| (p.clone(), format!("{title}  #{tag}"), String::new()))
            .collect();
        Ok(fuzzy_hits(&pairs, query, 50))
    }

    pub fn search_wiki(&self, query: &str) -> Result<Vec<FileHit>, Error> {
        if let Some((note_q, heading_q)) = query.split_once('#') {
            let notes = self.search_files(note_q.trim())?;
            let mut out = Vec::new();
            for n in notes {
                let headings: Vec<String> = {
                    let mut stmt = self.conn.prepare(
                        "SELECT h.text FROM headings h
                         JOIN files f ON f.id = h.note_id
                         WHERE f.path = ?1",
                    )?;
                    let collected = stmt
                        .query_map(params![n.path], |r| r.get(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    collected
                };
                let hq = heading_q.trim();
                for text in headings {
                    if hq.is_empty() || text.to_lowercase().contains(&hq.to_lowercase()) {
                        out.push(FileHit {
                            path: n.path.clone(),
                            title: format!("{}#{}", n.title, text),
                            snippet: None,
                            heading: Some(text),
                        });
                    }
                }
            }
            return Ok(out);
        }
        self.search_files(query)
    }
}

struct Walked {
    rel: PathBuf,
    abs: PathBuf,
    size: u64,
    mtime: u64,
    hash: String,
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<Walked>) -> Result<(), Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        let ft = entry.file_type()?;
        if skip_entry_name(&name, ft.is_dir()) {
            continue;
        }
        if ft.is_dir() {
            collect_files(root, &path, out)?;
            continue;
        }
        let meta = entry.metadata()?;
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let hash = hash_file(&path, size)?;
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        out.push(Walked {
            rel,
            abs: path,
            size,
            mtime,
            hash,
        });
    }
    Ok(())
}

fn hash_file(path: &Path, size: u64) -> Result<String, Error> {
    if size > FTS_MAX_BYTES {
        return Ok(format!("size:{size}"));
    }
    let bytes = fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn file_kind(rel: &Path) -> &'static str {
    let name = rel.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if name.ends_with(".md") {
        "note"
    } else if rel.components().any(|c| c.as_os_str() == "assets")
        || name.ends_with(".png")
        || name.ends_with(".jpg")
        || name.ends_with(".jpeg")
        || name.ends_with(".gif")
        || name.ends_with(".webp")
        || name.ends_with(".svg")
    {
        "asset"
    } else {
        "other"
    }
}

fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '/')
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .map(|t| format!("{t}*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn fuzzy_hits(rows: &[(String, String, String)], query: &str, limit: usize) -> Vec<FileHit> {
    if query.trim().is_empty() {
        return rows
            .iter()
            .take(limit)
            .map(|(p, t, _)| FileHit {
                path: p.clone(),
                title: t.clone(),
                snippet: None,
                heading: None,
            })
            .collect();
    }
    let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
    let mut needle_buf = Vec::new();
    let needle = nucleo_matcher::Utf32Str::new(query, &mut needle_buf);
    let mut scored: Vec<(u16, &str, &str)> = Vec::new();
    let mut hay_buf = Vec::new();
    for (path, title, extra) in rows {
        let blob = format!("{title} {path} {extra}");
        hay_buf.clear();
        let hay = nucleo_matcher::Utf32Str::new(&blob, &mut hay_buf);
        if let Some(score) = matcher.fuzzy_match(hay, needle) {
            scored.push((score, path.as_str(), title.as_str()));
        }
    }
    scored.sort_by_key(|b| std::cmp::Reverse(b.0));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, p, t)| FileHit {
            path: p.to_string(),
            title: t.to_string(),
            snippet: None,
            heading: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;

    #[test]
    fn rebuild_indexes_note_and_fts() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("vault");
        let vault = init(&root).expect("init");
        fs::write(
            root.join("notes/hello.md"),
            "---\ntitle: Hello\ntags: [inbox]\n---\n# Hello\n\nUniqueFTSToken here.\n",
        )
        .expect("write");
        let report = Index::rebuild(&vault).expect("rebuild");
        assert_eq!(report.notes_indexed, 1);
        let idx = Index::open(&vault.meta_dir).expect("open");
        let hits = idx.search_content("UniqueFTSToken").expect("search");
        assert!(hits.iter().any(|h| h.path.contains("hello.md")), "{hits:?}");
        let files = idx.search_files("hello").expect("files");
        assert!(files.iter().any(|h| h.path.contains("hello.md")));
        let tags = idx.search_tags("inbox").expect("tags");
        assert!(tags.iter().any(|h| h.path.contains("hello.md")));
    }

    #[test]
    fn search_files_matches_alias_and_frontmatter_title() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("vault");
        let vault = init(&root).expect("init");
        fs::write(
            root.join("notes/hello.md"),
            "---\ntitle: Greeting\naliases: [howdy, yo]\n---\n# Greeting\n",
        )
        .expect("write");
        Index::rebuild(&vault).expect("rebuild");
        let idx = Index::open(&vault.meta_dir).expect("open");
        let by_alias = idx.search_files("howdy").expect("alias");
        assert!(
            by_alias.iter().any(|h| h.path.contains("hello.md")),
            "{by_alias:?}"
        );
        let by_title = idx.search_files("Greeting").expect("title");
        assert!(
            by_title.iter().any(|h| h.title.contains("Greeting")),
            "{by_title:?}"
        );
    }
}
