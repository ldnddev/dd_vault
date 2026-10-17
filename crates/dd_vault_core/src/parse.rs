//! Note metadata extracted from markdown + YAML frontmatter.

use serde_yaml::Value;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedNote {
    pub title: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub headings: Vec<ParsedHeading>,
    pub links: Vec<ParsedLink>,
    pub body: String,
    pub frontmatter_json: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedHeading {
    pub level: i32,
    pub text: String,
    pub line: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Wiki,
    Md,
    Embed,
    /// `http`, `https`, or `mailto`. Opened in the default app, not as a note.
    Url,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLink {
    pub dst_raw: String,
    pub dst_heading: Option<String>,
    pub kind: LinkKind,
}

pub fn parse_note(rel: &Path, src: &str) -> ParsedNote {
    let stem = rel
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string();
    let (fm, body) = split_frontmatter(src);
    let mut aliases = Vec::new();
    let mut tags = Vec::new();
    let mut title_fm = None;
    let mut fm_json = "{}".to_string();
    if let Some(raw) = fm {
        if let Ok(val) = serde_yaml::from_str::<Value>(&raw) {
            fm_json = serde_json::to_string(&val).unwrap_or_else(|_| "{}".into());
            if let Some(t) = val.get("title").and_then(Value::as_str) {
                title_fm = Some(t.to_string());
            }
            aliases.extend(string_list(&val, "aliases"));
            tags.extend(string_list(&val, "tags"));
        }
    }
    let headings = parse_headings(body);
    let title = title_fm
        .or_else(|| {
            headings
                .iter()
                .find(|h| h.level == 1)
                .map(|h| h.text.clone())
        })
        .unwrap_or(stem);
    tags.extend(parse_hash_tags(body));
    tags.sort();
    tags.dedup();
    let mut links = parse_wikilinks(body);
    links.extend(parse_md_links(body));
    ParsedNote {
        title,
        aliases,
        tags,
        headings,
        links,
        body: body.to_string(),
        frontmatter_json: fm_json,
    }
}

fn string_list(val: &Value, key: &str) -> Vec<String> {
    match val.get(key) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn split_frontmatter(src: &str) -> (Option<String>, &str) {
    let Some(rest) = src.strip_prefix("---") else {
        return (None, src);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    if let Some(end) = rest.find("\n---") {
        let fm = rest[..end].to_string();
        let after = rest[end + 4..]
            .strip_prefix('\n')
            .unwrap_or(&rest[end + 4..]);
        return (Some(fm), after);
    }
    (None, src)
}

fn parse_headings(body: &str) -> Vec<ParsedHeading> {
    body.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let line = line.trim_end();
            let hashes = line.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
                let text = line[hashes + 1..].trim().to_string();
                if text.is_empty() {
                    return None;
                }
                Some(ParsedHeading {
                    level: hashes as i32,
                    text,
                    line: (i + 1) as i32,
                })
            } else {
                None
            }
        })
        .collect()
}

fn parse_hash_tags(body: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'#' {
            let start_ok = i == 0 || bytes[i - 1].is_ascii_whitespace();
            if start_ok {
                let mut j = i + 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric()
                        || bytes[j] == b'_'
                        || bytes[j] == b'-'
                        || bytes[j] == b'/')
                {
                    j += 1;
                }
                if j > i + 1 {
                    if let Ok(tag) = std::str::from_utf8(&bytes[i + 1..j]) {
                        tags.push(tag.to_string());
                    }
                    i = j;
                    continue;
                }
            }
        }
        i += 1;
    }
    tags
}

/// A wikilink or local markdown link with char offsets into `src`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedLink {
    pub start: usize,
    pub end: usize,
    pub link: ParsedLink,
}

/// Link covering `cursor` (char index), if any.
pub fn link_at(src: &str, cursor: usize) -> Option<ParsedLink> {
    located_links(src)
        .into_iter()
        .find(|l| cursor >= l.start && cursor < l.end)
        .map(|l| l.link)
}

pub fn located_links(src: &str) -> Vec<LocatedLink> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        if i + 1 < n && chars[i] == '[' && chars[i + 1] == '[' {
            let embed = i > 0 && chars[i - 1] == '!';
            if let Some(rel) = chars[i + 2..]
                .windows(2)
                .position(|w| w[0] == ']' && w[1] == ']')
            {
                let inner_end = i + 2 + rel;
                let inner: String = chars[i + 2..inner_end].iter().collect();
                let start = if embed { i - 1 } else { i };
                let end = inner_end + 2;
                if let Some(link) = parse_wiki_target(&inner, embed) {
                    out.push(LocatedLink { start, end, link });
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    i = 0;
    while i < n {
        if chars[i] == '[' && !(i + 1 < n && chars[i + 1] == '[') {
            if let Some(br) = chars[i + 1..].iter().position(|&c| c == ']') {
                let close = i + 1 + br;
                if close + 1 < n && chars[close + 1] == '(' {
                    if let Some(pr) = chars[close + 2..].iter().position(|&c| c == ')') {
                        let url_e = close + 2 + pr;
                        let url: String = chars[close + 2..url_e].iter().collect();
                        let url = url.trim();
                        if let Some(link) = parse_md_target(url) {
                            out.push(LocatedLink {
                                start: i,
                                end: url_e + 1,
                                link,
                            });
                        }
                        i = url_e + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out.extend(scan_bare_urls(&chars, &out));
    out.sort_by_key(|l| l.start);
    out
}

pub fn is_external_url(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

fn parse_wiki_target(inner: &str, embed: bool) -> Option<ParsedLink> {
    let (target, _alias) = match inner.split_once('|') {
        Some((t, a)) => (t.trim(), Some(a.trim())),
        None => (inner.trim(), None),
    };
    let (dst, heading) = match target.split_once('#') {
        Some((d, h)) => (d.trim().to_string(), Some(h.trim().to_string())),
        None => (target.to_string(), None),
    };
    if dst.is_empty() {
        return None;
    }
    Some(ParsedLink {
        dst_raw: dst,
        dst_heading: heading,
        kind: if embed {
            LinkKind::Embed
        } else {
            LinkKind::Wiki
        },
    })
}

fn parse_md_target(url: &str) -> Option<ParsedLink> {
    if url.starts_with('#') {
        return None;
    }
    if is_external_url(url) {
        return Some(ParsedLink {
            dst_raw: url.to_string(),
            dst_heading: None,
            kind: LinkKind::Url,
        });
    }
    let (dst, heading) = match url.split_once('#') {
        Some((d, h)) => (d.trim().to_string(), Some(h.trim().to_string())),
        None => (url.to_string(), None),
    };
    if dst.ends_with(".md") || dst.contains('/') || !dst.contains(':') {
        Some(ParsedLink {
            dst_raw: dst,
            dst_heading: heading,
            kind: LinkKind::Md,
        })
    } else {
        None
    }
}

fn scan_bare_urls(chars: &[char], existing: &[LocatedLink]) -> Vec<LocatedLink> {
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        let scheme_len = bare_scheme_len(&chars[i..]);
        if scheme_len == 0 || existing.iter().any(|l| i >= l.start && i < l.end) {
            i += 1;
            continue;
        }
        let mut j = i + scheme_len;
        while j < n {
            let c = chars[j];
            if c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '|') {
                break;
            }
            j += 1;
        }
        j = trim_url_tail(chars, i, j);
        if j > i + scheme_len {
            let url: String = chars[i..j].iter().collect();
            out.push(LocatedLink {
                start: i,
                end: j,
                link: ParsedLink {
                    dst_raw: url,
                    dst_heading: None,
                    kind: LinkKind::Url,
                },
            });
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn bare_scheme_len(chars: &[char]) -> usize {
    let head: String = chars.iter().take(8).collect();
    let head = head.to_ascii_lowercase();
    if head.starts_with("https://") {
        8
    } else if head.starts_with("http://") || head.starts_with("mailto:") {
        7
    } else {
        0
    }
}

fn trim_url_tail(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        let c = chars[end - 1];
        if matches!(c, '.' | ',' | ';' | '!' | '?') {
            end -= 1;
            continue;
        }
        if c == ')' {
            let opens = chars[start..end].iter().filter(|ch| **ch == '(').count();
            let closes = chars[start..end].iter().filter(|ch| **ch == ')').count();
            if closes > opens {
                end -= 1;
                continue;
            }
        }
        break;
    }
    end
}

fn parse_wikilinks(body: &str) -> Vec<ParsedLink> {
    located_links(body)
        .into_iter()
        .filter(|l| matches!(l.link.kind, LinkKind::Wiki | LinkKind::Embed))
        .map(|l| l.link)
        .collect()
}

fn parse_md_links(body: &str) -> Vec<ParsedLink> {
    let mut links = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let url = rest[..end].trim();
        rest = &rest[end + 1..];
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with('#') {
            continue;
        }
        let (dst, heading) = match url.split_once('#') {
            Some((d, h)) => (d.trim().to_string(), Some(h.trim().to_string())),
            None => (url.to_string(), None),
        };
        if dst.ends_with(".md") || dst.contains('/') || !dst.contains(':') {
            links.push(ParsedLink {
                dst_raw: dst,
                dst_heading: heading,
                kind: LinkKind::Md,
            });
        }
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn title_from_frontmatter_then_h1() {
        let p = parse_note(
            Path::new("n.md"),
            "---\ntitle: FM\naliases:\n  - Alt\ntags: [a, b]\n---\n# Heading\n#tagC\n",
        );
        assert_eq!(p.title, "FM");
        assert_eq!(p.aliases, vec!["Alt"]);
        assert!(p.tags.contains(&"a".to_string()));
        assert!(p.tags.contains(&"tagC".to_string()));
        assert_eq!(p.headings[0].text, "Heading");
    }

    #[test]
    fn wikilinks_and_embeds() {
        let p = parse_note(
            Path::new("n.md"),
            "See [[Inbox#Tasks|box]] and ![[Other]] and [x](foo.md).",
        );
        assert!(p
            .links
            .iter()
            .any(|l| l.kind == LinkKind::Wiki && l.dst_raw == "Inbox"));
        assert!(p
            .links
            .iter()
            .any(|l| l.kind == LinkKind::Embed && l.dst_raw == "Other"));
        assert!(p
            .links
            .iter()
            .any(|l| l.kind == LinkKind::Md && l.dst_raw == "foo.md"));
    }

    #[test]
    fn link_at_finds_wiki_and_markdown() {
        let src = "See [[Inbox#Tasks|box]] and [x](foo.md).";
        let wiki = link_at(src, src.find("Inbox").expect("inbox")).expect("wiki");
        assert_eq!(wiki.dst_raw, "Inbox");
        assert_eq!(wiki.dst_heading.as_deref(), Some("Tasks"));
        let md = link_at(src, src.find("foo.md").expect("md")).expect("md");
        assert_eq!(md.dst_raw, "foo.md");
        assert!(link_at(src, 0).is_none());
    }

    #[test]
    fn link_at_finds_urls() {
        let src = "See [docs](https://example.com/a) and https://example.com/b.";
        let labeled = link_at(src, src.find("docs").unwrap()).unwrap();
        assert_eq!(labeled.kind, LinkKind::Url);
        assert_eq!(labeled.dst_raw, "https://example.com/a");
        let bare = link_at(src, src.find("example.com/b").unwrap()).unwrap();
        assert_eq!(bare.dst_raw, "https://example.com/b");
        assert!(!bare.dst_raw.ends_with('.'));
    }
}
