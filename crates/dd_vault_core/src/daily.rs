//! Daily notes: `notes/daily/YYYY-MM-DD.md` (path overridable).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Error;

pub fn today_ymd() -> String {
    if let Ok(out) = Command::new("date").arg("+%Y-%m-%d").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') {
                return s;
            }
        }
    }
    unix_utc_ymd()
}

fn unix_utc_ymd() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil date from Unix days (Howard Hinnant algorithm).
    let z = (secs / 86400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn daily_rel(daily_dir: &str, ymd: &str) -> PathBuf {
    let dir = daily_dir.trim().trim_matches('/');
    let dir = if dir.is_empty() { "notes/daily" } else { dir };
    PathBuf::from(dir).join(format!("{ymd}.md"))
}

pub fn ensure_daily(root: &Path, daily_dir: &str, ymd: &str) -> Result<PathBuf, Error> {
    let rel = daily_rel(daily_dir, ymd);
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidEntryName(rel.display().to_string()));
    }
    let abs = root.join(&rel);
    if abs.is_file() {
        return Ok(rel);
    }
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = format!("---\ntitle: {ymd}\ndate: {ymd}\n---\n\n# {ymd}\n\n");
    fs::write(&abs, body)?;
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;

    #[test]
    fn ensure_daily_creates_once() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("notes");
        init(&root).expect("init");
        let rel = ensure_daily(&root, "notes/daily", "2026-09-19").expect("daily");
        assert_eq!(rel, PathBuf::from("notes/daily/2026-09-19.md"));
        let abs = root.join(&rel);
        let body = fs::read_to_string(&abs).expect("read");
        assert!(body.contains("title: 2026-09-19"), "{body}");
        fs::write(&abs, "keep\n").unwrap();
        let again = ensure_daily(&root, "notes/daily", "2026-09-19").expect("again");
        assert_eq!(again, rel);
        assert_eq!(fs::read_to_string(&abs).unwrap(), "keep\n");
    }

    #[test]
    fn today_ymd_shape() {
        let d = today_ymd();
        assert_eq!(d.len(), 10, "{d}");
        assert_eq!(&d[4..5], "-");
        assert_eq!(&d[7..8], "-");
    }
}
