//! Unicode Mermaid preview: flowcharts, sequence, pie. No mermaid-cli.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Lr,
    Td,
}

#[derive(Clone, Debug)]
struct FlowNode {
    id: String,
    label: String,
}

#[derive(Clone, Debug)]
struct FlowEdge {
    from: String,
    to: String,
    label: String,
}

#[derive(Clone, Debug)]
struct SeqMsg {
    from: String,
    to: String,
    text: String,
}

#[derive(Clone, Debug)]
enum Diagram {
    Flow {
        dir: Dir,
        nodes: Vec<FlowNode>,
        edges: Vec<FlowEdge>,
    },
    Sequence {
        actors: Vec<String>,
        messages: Vec<SeqMsg>,
    },
    Pie {
        title: String,
        slices: Vec<(String, f64)>,
    },
}

pub fn render_mermaid(src: &str, max_width: usize) -> Result<Vec<String>, String> {
    let cleaned = strip_noise(src);
    let diagram = parse(&cleaned)?;
    let mut lines = match diagram {
        Diagram::Flow { dir, nodes, edges } => render_flow(dir, &nodes, &edges, max_width.max(8)),
        Diagram::Sequence { actors, messages } => {
            render_sequence(&actors, &messages, max_width.max(8))
        }
        Diagram::Pie { title, slices } => render_pie(&title, &slices, max_width.max(12)),
    };
    if lines.is_empty() {
        return Err("empty diagram".into());
    }
    let max_width = max_width.max(8);
    for line in &mut lines {
        let n = line.chars().count();
        if n > max_width {
            *line = line
                .chars()
                .take(max_width.saturating_sub(1))
                .collect::<String>()
                + "…";
        }
    }
    Ok(lines)
}

fn strip_noise(src: &str) -> String {
    let mut s = src.replace("\r\n", "\n").replace('\r', "\n");
    while let Some(start) = s.find("%%{") {
        if let Some(rel) = s[start + 3..].find("}%%") {
            s.replace_range(start..start + 3 + rel + 3, " ");
        } else {
            break;
        }
    }
    let mut out = String::new();
    for line in s.lines() {
        let cut = line.find("%%").unwrap_or(line.len());
        out.push_str(line[..cut].trim_end());
        out.push('\n');
    }
    out
}

fn parse(src: &str) -> Result<Diagram, String> {
    let mut lines: Vec<&str> = src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return Err("empty diagram".into());
    }
    if lines[0] == "---" {
        if let Some(end) = lines.iter().skip(1).position(|l| *l == "---") {
            lines.drain(0..=end + 1);
        }
    }
    if lines.is_empty() {
        return Err("empty diagram".into());
    }
    let header = lines[0];
    let lower = header.to_ascii_lowercase();
    if lower.starts_with("sequencediagram") {
        return parse_sequence(&lines[1..]);
    }
    if lower == "pie" || lower.starts_with("pie ") {
        let title = header
            .split_once("title")
            .map(|(_, t)| t.trim().to_string())
            .unwrap_or_default();
        return parse_pie(title, &lines[1..]);
    }
    if lower.starts_with("statediagram") {
        return parse_flow(Dir::Td, &lines[1..]);
    }
    if lower.starts_with("flowchart") || lower.starts_with("graph ") || lower == "graph" {
        let dir = parse_dir(header).unwrap_or(Dir::Td);
        return parse_flow(dir, &lines[1..]);
    }
    let known = [
        "classdiagram",
        "erdiagram",
        "gantt",
        "journey",
        "gitgraph",
        "mindmap",
        "timeline",
        "quadrantchart",
        "requirementdiagram",
        "c4context",
        "sankey-beta",
        "xychart-beta",
        "block-beta",
        "kanban",
        "architecture-beta",
    ];
    let kind = lower.split_whitespace().next().unwrap_or("diagram");
    if known.iter().any(|k| kind.starts_with(k)) {
        return Err(format!("{kind} is not drawn in preview"));
    }
    parse_flow(Dir::Td, &lines)
}

fn parse_dir(header: &str) -> Option<Dir> {
    let tok = header.split_whitespace().nth(1)?.to_ascii_uppercase();
    match tok.as_str() {
        "LR" | "RL" => Some(Dir::Lr),
        "TD" | "TB" | "BT" | "DT" => Some(Dir::Td),
        _ => None,
    }
}

fn parse_flow(mut dir: Dir, lines: &[&str]) -> Result<Diagram, String> {
    let mut nodes: Vec<FlowNode> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: Vec<FlowEdge> = Vec::new();
    let mut skip_end = 0usize;
    for line in lines {
        for stmt in split_stmts(line) {
            let s = stmt.trim();
            if s.is_empty() {
                continue;
            }
            let low = s.to_ascii_lowercase();
            if skip_end > 0 {
                if low == "end" {
                    skip_end -= 1;
                } else if low.starts_with("subgraph") {
                    skip_end += 1;
                } else {
                    parse_flow_stmt(s, &mut dir, &mut nodes, &mut index, &mut edges);
                }
                continue;
            }
            if low.starts_with("subgraph") {
                skip_end = 1;
                continue;
            }
            if matches!(
                low.split_whitespace().next(),
                Some("classdef" | "class" | "click" | "style" | "linkstyle" | "end")
            ) {
                continue;
            }
            if low.starts_with("direction ") {
                if let Some(d) = parse_dir(s) {
                    dir = d;
                }
                continue;
            }
            parse_flow_stmt(s, &mut dir, &mut nodes, &mut index, &mut edges);
        }
    }
    if nodes.is_empty() {
        return Err("no nodes in flowchart".into());
    }
    Ok(Diagram::Flow { dir, nodes, edges })
}

fn parse_flow_stmt(
    s: &str,
    _dir: &mut Dir,
    nodes: &mut Vec<FlowNode>,
    index: &mut HashMap<String, usize>,
    edges: &mut Vec<FlowEdge>,
) {
    if let Some((left, arrow, right, elabel)) = split_edge(s) {
        let a = upsert_node(left, nodes, index);
        let b = upsert_node(right, nodes, index);
        edges.push(FlowEdge {
            from: a,
            to: b,
            label: elabel,
        });
        let _ = arrow;
        return;
    }
    let _ = upsert_node(s, nodes, index);
}

fn upsert_node(raw: &str, nodes: &mut Vec<FlowNode>, index: &mut HashMap<String, usize>) -> String {
    let (id, label) = parse_node_ref(raw);
    if let Some(&i) = index.get(&id) {
        if label != id && nodes[i].label == nodes[i].id {
            nodes[i].label = label;
        }
        return id;
    }
    index.insert(id.clone(), nodes.len());
    nodes.push(FlowNode {
        id: id.clone(),
        label,
    });
    id
}

fn parse_node_ref(raw: &str) -> (String, String) {
    let s = raw.trim();
    let mut i = 0usize;
    let bytes = s.as_bytes();
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
        let q = bytes[i];
        i += 1;
        let qs = i;
        while i < bytes.len() && bytes[i] != q {
            i += 1;
        }
        let id = s[qs..i].to_string();
        if i < bytes.len() {
            i += 1;
        }
        let label = parse_shape_label(&s[i..]).unwrap_or_else(|| id.clone());
        return (id, label);
    }
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
            i += 1;
        } else {
            break;
        }
    }
    if i == start {
        return (s.to_string(), s.to_string());
    }
    let id = s[start..i].to_string();
    if id == "*" || id == "[*]" {
        return ("*".into(), "●".into());
    }
    let rest = s[i..].trim_start();
    let label = parse_shape_label(rest).unwrap_or_else(|| id.clone());
    (id, label)
}

fn parse_shape_label(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    let b = rest.as_bytes();
    if b.len() >= 2 && &b[..2] == b"((" {
        return take_until(rest, 2, "))");
    }
    if b.len() >= 2 && &b[..2] == b"[[" {
        return take_until(rest, 2, "]]");
    }
    if b.len() >= 2 && &b[..2] == b"([" {
        return take_until(rest, 2, "])");
    }
    if b.len() >= 2 && &b[..2] == b"[(" {
        return take_until(rest, 2, ")]");
    }
    if b.len() >= 2 && &b[..2] == b"{{" {
        return take_until(rest, 2, "}}");
    }
    if rest.starts_with('[') {
        return take_until(rest, 1, "]");
    }
    if rest.starts_with('(') {
        return take_until(rest, 1, ")");
    }
    if rest.starts_with('{') {
        return take_until(rest, 1, "}");
    }
    if rest.starts_with('>') {
        return take_until(rest, 1, "]");
    }
    None
}

fn take_until(s: &str, skip: usize, end: &str) -> Option<String> {
    let rest = s.get(skip..)?;
    let inner = if rest.starts_with('"') || rest.starts_with('\'') {
        let q = rest.chars().next()?;
        let body = rest.get(1..)?;
        let close = body.find(q)?;
        body[..close].to_string()
    } else {
        let close = rest.find(end)?;
        rest[..close].to_string()
    };
    Some(unquote(&inner))
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"') || t.starts_with('\'') && t.ends_with('\''))
        && t.len() >= 2
    {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn split_edge(s: &str) -> Option<(&str, &str, &str, String)> {
    const ARROWS: &[&str] = &["-.->", "<-->", "==>", "-->", "---", "--x"];
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            for &arr in ARROWS {
                if s[i..].starts_with(arr) {
                    let left = s[..i].trim();
                    let mut rest = &s[i + arr.len()..];
                    let mut label = String::new();
                    rest = rest.trim_start();
                    if rest.starts_with('|') {
                        if let Some(end) = rest[1..].find('|') {
                            label = rest[1..1 + end].trim().to_string();
                            rest = rest[2 + end..].trim_start();
                        }
                    }
                    let right = rest.trim();
                    if !left.is_empty() && !right.is_empty() {
                        return Some((left, arr, right, label));
                    }
                }
            }
            if s[i..].starts_with("-- ") {
                if let Some(end) = s[i + 3..].find(" -->") {
                    let left = s[..i].trim();
                    let label = s[i + 3..i + 3 + end].trim().to_string();
                    let right = s[i + 3 + end + 4..].trim();
                    if !left.is_empty() && !right.is_empty() {
                        return Some((left, "-->", right, label));
                    }
                }
            }
        }
        i += 1;
    }
    None
}

fn split_stmts(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        match c {
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => depth -= 1,
            b';' if depth == 0 => {
                out.push(line[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(line[start..].trim());
    out
}

fn parse_sequence(lines: &[&str]) -> Result<Diagram, String> {
    let mut actors: Vec<String> = Vec::new();
    let mut messages: Vec<SeqMsg> = Vec::new();
    for line in lines {
        let low = line.to_ascii_lowercase();
        if let Some(rest) = low
            .strip_prefix("participant ")
            .or_else(|| low.strip_prefix("actor "))
        {
            let name = line[line.len() - rest.len()..].trim();
            let name = name.split_once(" as ").map(|(a, _)| a).unwrap_or(name);
            push_unique(&mut actors, unquote(name.trim()));
            continue;
        }
        if matches!(
            low.split_whitespace().next(),
            Some(
                "note"
                    | "loop"
                    | "alt"
                    | "else"
                    | "end"
                    | "opt"
                    | "par"
                    | "and"
                    | "rect"
                    | "activate"
                    | "deactivate"
                    | "autonumber"
            )
        ) {
            continue;
        }
        if let Some((from, to, text)) = parse_seq_msg(line) {
            push_unique(&mut actors, from.clone());
            push_unique(&mut actors, to.clone());
            messages.push(SeqMsg { from, to, text });
        }
    }
    if actors.is_empty() {
        return Err("no actors in sequence diagram".into());
    }
    Ok(Diagram::Sequence { actors, messages })
}

fn parse_seq_msg(line: &str) -> Option<(String, String, String)> {
    let (left, text) = line.split_once(':')?;
    const ARROWS: &[&str] = &["-->>", "->>", "-->", "->", "--x", "-x", "--)"];
    for &arr in ARROWS {
        if let Some(idx) = left.find(arr) {
            let from = unquote(left[..idx].trim());
            let to = unquote(left[idx + arr.len()..].trim());
            if !from.is_empty() && !to.is_empty() {
                return Some((from, to, text.trim().to_string()));
            }
        }
    }
    None
}

fn parse_pie(title: String, lines: &[&str]) -> Result<Diagram, String> {
    let mut slices = Vec::new();
    let mut title = title;
    for line in lines {
        let low = line.to_ascii_lowercase();
        if let Some(rest) = low.strip_prefix("title ") {
            title = line[line.len() - rest.len()..].trim().to_string();
            continue;
        }
        if let Some((label, val)) = line.rsplit_once(':') {
            if let Ok(n) = val.trim().trim_end_matches('%').parse::<f64>() {
                slices.push((unquote(label), n));
            }
        }
    }
    if slices.is_empty() {
        return Err("no slices in pie".into());
    }
    Ok(Diagram::Pie { title, slices })
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.iter().any(|x| x == &s) {
        v.push(s);
    }
}

fn box_w(label: &str) -> usize {
    label.chars().count().saturating_add(4).max(5)
}

fn render_flow(dir: Dir, nodes: &[FlowNode], edges: &[FlowEdge], max_width: usize) -> Vec<String> {
    let lines = layout_flow(dir, nodes, edges);
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    if dir == Dir::Lr && w > max_width {
        return layout_flow(Dir::Td, nodes, edges);
    }
    lines
}

fn layout_flow(dir: Dir, nodes: &[FlowNode], edges: &[FlowEdge]) -> Vec<String> {
    let ranks = assign_ranks(nodes, edges);
    let max_rank = ranks.values().copied().max().unwrap_or(0);
    let mut by_rank: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, n) in nodes.iter().enumerate() {
        by_rank[ranks[&n.id]].push(i);
    }

    const BOX_H: usize = 3;
    let gap = 3usize;
    let mut cells: HashMap<String, Cell> = HashMap::new();

    match dir {
        Dir::Lr => {
            let mut x = 0usize;
            for rank_nodes in &by_rank {
                let col_w = rank_nodes
                    .iter()
                    .map(|&i| box_w(&nodes[i].label))
                    .max()
                    .unwrap_or(5);
                let mut y = 0usize;
                for &i in rank_nodes {
                    let w = box_w(&nodes[i].label);
                    cells.insert(nodes[i].id.clone(), Cell { x, y, w, h: BOX_H });
                    y += BOX_H + 1;
                }
                x += col_w + gap;
            }
        }
        Dir::Td => {
            let mut y = 0usize;
            for rank_nodes in &by_rank {
                let mut x = 0usize;
                for &i in rank_nodes {
                    let w = box_w(&nodes[i].label);
                    cells.insert(nodes[i].id.clone(), Cell { x, y, w, h: BOX_H });
                    x += w + 2;
                }
                y += BOX_H + gap;
            }
        }
    }

    let mut gw = 1usize;
    let mut gh = 1usize;
    for n in nodes {
        let c = &cells[&n.id];
        gw = gw.max(c.x + c.w);
        gh = gh.max(c.y + c.h);
    }
    for e in edges {
        if let Some(c) = cells.get(&e.from) {
            gw = gw.max(c.x + c.w + gap);
            gh = gh.max(c.y + BOX_H + gap);
        }
        if let Some(c) = cells.get(&e.to) {
            gw = gw.max(c.x + 8);
            gh = gh.max(c.y + BOX_H);
        }
    }
    gw = gw.clamp(8, 240);
    gh = gh.clamp(3, 120);

    let mut g = Grid::new(gw, gh);
    for n in nodes {
        let c = cells[&n.id];
        g.draw_box(c.x, c.y, c.w, &n.label);
    }
    for e in edges {
        let Some(&from) = cells.get(&e.from) else {
            continue;
        };
        let Some(&to) = cells.get(&e.to) else {
            continue;
        };
        match dir {
            Dir::Lr => g.connect_lr(from, to, &e.label),
            Dir::Td => g.connect_td(from, to, &e.label),
        }
    }
    g.lines()
}

#[derive(Clone, Copy)]
struct Cell {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

fn assign_ranks(nodes: &[FlowNode], edges: &[FlowEdge]) -> HashMap<String, usize> {
    let mut rank: HashMap<String, usize> = HashMap::new();
    let mut remaining: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let mut guard = 0usize;
    while !remaining.is_empty() && guard < nodes.len() + 2 {
        guard += 1;
        let mut progressed = false;
        remaining.retain(|id| {
            let preds: Vec<&FlowEdge> = edges.iter().filter(|e| e.to == *id).collect();
            if preds.is_empty() {
                rank.entry(id.clone()).or_insert(0);
                progressed = true;
                return false;
            }
            if preds.iter().all(|e| rank.contains_key(&e.from)) {
                let r = preds.iter().map(|e| rank[&e.from] + 1).max().unwrap_or(0);
                rank.insert(id.clone(), r);
                progressed = true;
                false
            } else {
                true
            }
        });
        if !progressed {
            if let Some(id) = remaining.first().cloned() {
                let r = edges
                    .iter()
                    .filter(|e| e.to == id)
                    .filter_map(|e| rank.get(&e.from).map(|n| n + 1))
                    .max()
                    .unwrap_or(0);
                rank.insert(id.clone(), r);
                remaining.remove(0);
            }
        }
    }
    for n in nodes {
        rank.entry(n.id.clone()).or_insert(0);
    }
    rank
}

fn render_sequence(actors: &[String], messages: &[SeqMsg], max_width: usize) -> Vec<String> {
    let col_w: Vec<usize> = actors.iter().map(|a| box_w(a).max(8)).collect();
    let gap = 3usize;
    let mut x_of: Vec<usize> = Vec::new();
    let mut x = 0usize;
    for (i, w) in col_w.iter().enumerate() {
        x_of.push(x);
        x += w + if i + 1 == col_w.len() { 0 } else { gap };
    }
    let width = x.max(8).min(max_width.max(x).min(240));
    let body_h = messages.len().saturating_mul(2).saturating_add(4);
    let mut g = Grid::new(width.max(x).max(8), body_h.clamp(5, 80));
    for (i, a) in actors.iter().enumerate() {
        g.draw_box(x_of[i], 0, col_w[i], a);
        let cx = x_of[i] + col_w[i] / 2;
        for y in 3..g.h {
            g.merge(cx, y, '│');
        }
    }
    let mut y = 4usize;
    for m in messages {
        let Some(i) = actors.iter().position(|a| a == &m.from) else {
            continue;
        };
        let Some(j) = actors.iter().position(|a| a == &m.to) else {
            continue;
        };
        let c1 = x_of[i] + col_w[i] / 2;
        let c2 = x_of[j] + col_w[j] / 2;
        let (left, right, fwd) = if c1 <= c2 {
            (c1, c2, true)
        } else {
            (c2, c1, false)
        };
        if !m.text.is_empty() && y < g.h {
            let tx = left.saturating_add(1).min(g.w.saturating_sub(1));
            g.put_str(tx, y.saturating_sub(1).max(3), &m.text);
        }
        if y < g.h {
            for x in left..=right {
                g.merge(x, y, '─');
            }
            g.merge(left, y, '│');
            g.merge(right, y, '│');
            if fwd {
                g.merge(right.saturating_sub(1).max(left), y, '►');
            } else {
                g.merge(left.saturating_add(1).min(right), y, '◄');
            }
        }
        y += 2;
    }
    g.lines()
}

fn render_pie(title: &str, slices: &[(String, f64)], max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if !title.is_empty() {
        lines.push(title.to_string());
    }
    let max_v = slices.iter().map(|(_, v)| *v).fold(0.0_f64, f64::max);
    let bar_room = max_width.saturating_sub(16).max(4);
    for (label, v) in slices {
        let n = if max_v <= 0.0 {
            0
        } else {
            ((v / max_v) * bar_room as f64).round() as usize
        };
        let bar: String = std::iter::repeat_n('█', n.max(1)).collect();
        let val = if v.fract() == 0.0 {
            format!("{v:.0}")
        } else {
            format!("{v}")
        };
        lines.push(format!("{label:<8} {bar} {val}"));
    }
    lines
}

struct Grid {
    w: usize,
    h: usize,
    cells: Vec<Vec<char>>,
}

impl Grid {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            cells: vec![vec![' '; w]; h],
        }
    }

    fn put(&mut self, x: usize, y: usize, c: char) {
        if x < self.w && y < self.h {
            self.cells[y][x] = c;
        }
    }

    fn merge(&mut self, x: usize, y: usize, c: char) {
        if x >= self.w || y >= self.h {
            return;
        }
        let old = self.cells[y][x];
        self.cells[y][x] = merge_box(old, c);
    }

    fn put_str(&mut self, x: usize, y: usize, s: &str) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i, y, ch);
        }
    }

    fn draw_box(&mut self, x: usize, y: usize, w: usize, label: &str) {
        let w = w.max(3);
        self.put(x, y, '┌');
        self.put(x + w - 1, y, '┐');
        self.put(x, y + 2, '└');
        self.put(x + w - 1, y + 2, '┘');
        for i in 1..w - 1 {
            self.put(x + i, y, '─');
            self.put(x + i, y + 2, '─');
        }
        self.put(x, y + 1, '│');
        self.put(x + w - 1, y + 1, '│');
        let inner = w.saturating_sub(2);
        let lab: String = label.chars().take(inner).collect();
        let pad = inner.saturating_sub(lab.chars().count());
        let left = pad / 2;
        self.put_str(x + 1 + left, y + 1, &lab);
    }

    fn connect_lr(&mut self, from: Cell, to: Cell, label: &str) {
        let y_a = from.y + 1;
        let y_b = to.y + 1;
        let x0 = from.x + from.w;
        let x2 = to.x;
        if x2 == 0 {
            return;
        }
        let x_tip = x2.saturating_sub(1);
        if y_a == y_b {
            for x in x0..x_tip {
                self.merge(x, y_a, '─');
            }
            self.merge(x_tip, y_a, '►');
            if !label.is_empty() && y_a > 0 {
                self.put_str(x0, y_a - 1, label);
            }
            return;
        }
        self.merge(x0, y_a, '─');
        let bus = x0.saturating_add(1).min(x_tip);
        self.merge(bus, y_a, if y_b > y_a { '┐' } else { '┘' });
        let (lo, hi) = if y_a < y_b { (y_a, y_b) } else { (y_b, y_a) };
        for y in lo + 1..hi {
            self.merge(bus, y, '│');
        }
        self.merge(bus, y_b, if y_b > y_a { '└' } else { '┌' });
        for x in bus + 1..x_tip {
            self.merge(x, y_b, '─');
        }
        self.merge(x_tip, y_b, '►');
        if !label.is_empty() {
            self.put_str(x0, lo, label);
        }
    }

    fn connect_td(&mut self, from: Cell, to: Cell, label: &str) {
        let cx1 = from.x + from.w / 2;
        let cx2 = to.x + to.w / 2;
        let y0 = from.y + from.h - 1;
        let y2 = to.y;
        self.merge(cx1, y0, '┬');
        if cx1 == cx2 {
            for y in y0 + 1..y2 {
                self.merge(cx1, y, '│');
            }
            if y2 > 0 {
                self.merge(cx1, y2.saturating_sub(1), '▼');
                self.merge(cx2, y2, '┴');
            }
            if !label.is_empty() {
                self.put_str(cx1.saturating_add(1), y0 + 1, label);
            }
            return;
        }
        let bus_y = y0 + 1;
        self.merge(cx1, bus_y, '│');
        let turn_y = (bus_y + 1).min(y2.saturating_sub(1));
        let (left, right) = if cx1 < cx2 { (cx1, cx2) } else { (cx2, cx1) };
        self.merge(cx1, turn_y, if cx2 > cx1 { '└' } else { '┘' });
        for x in left + 1..right {
            self.merge(x, turn_y, '─');
        }
        self.merge(cx2, turn_y, if cx2 > cx1 { '┐' } else { '┌' });
        for y in turn_y + 1..y2 {
            self.merge(cx2, y, '│');
        }
        if y2 > 0 {
            self.merge(cx2, y2.saturating_sub(1), '▼');
            self.merge(cx2, y2, '┴');
        }
        if !label.is_empty() {
            self.put_str(left.saturating_add(1), turn_y.saturating_sub(1), label);
        }
    }

    fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .cells
            .iter()
            .map(|row| {
                let s: String = row.iter().collect();
                s.trim_end().to_string()
            })
            .collect();
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        out
    }
}

fn merge_box(old: char, new: char) -> char {
    if old == ' ' || old == new {
        return new;
    }
    let h = matches!(
        old,
        '─' | '┌' | '┐' | '└' | '┘' | '┬' | '┴' | '┼' | '►' | '◄'
    ) || matches!(new, '─' | '►' | '◄');
    let v = matches!(
        old,
        '│' | '┌' | '┐' | '└' | '┘' | '┤' | '├' | '┬' | '┴' | '┼' | '▼'
    ) || matches!(new, '│' | '▼');
    match (old, new) {
        ('│', '─') | ('─', '│') => '┼',
        ('│', '►') => '├',
        ('│', '┬') | ('─', '┬') => '┬',
        ('│', '┴') | ('─', '┴') | ('┌', '┴') | ('┐', '┴') => '┴',
        ('└', '─') | ('─', '└') => '└',
        ('┘', '─') | ('─', '┘') => '┘',
        ('┌', '─') | ('─', '┌') => '┌',
        ('┐', '─') | ('─', '┐') => '┐',
        ('│', '┐') => '┤',
        ('│', '┌') => '├',
        _ if h && v => '┼',
        _ => new,
    }
}

#[cfg(test)]
mod tests {
    use super::render_mermaid;

    fn dump(src: &str) -> String {
        render_mermaid(src, 80).unwrap().join("\n")
    }

    #[test]
    fn flowchart_lr_chain() {
        let d =
            dump("flowchart LR\n  Open --> Preview\n  Preview --> Cache\n  Cache --> HalfBlocks\n");
        assert!(d.contains("Open"), "{d}");
        assert!(d.contains("Preview"), "{d}");
        assert!(d.contains("Cache"), "{d}");
        assert!(d.contains("HalfBlocks"), "{d}");
        assert!(d.contains('►'), "{d}");
        assert!(d.contains('┌'), "{d}");
    }

    #[test]
    fn flowchart_td_and_labels() {
        let d = dump("graph TD\n  A[Start] --> B{Go}\n  B --> C[End]\n");
        assert!(d.contains("Start"), "{d}");
        assert!(d.contains("Go"), "{d}");
        assert!(d.contains("End"), "{d}");
        assert!(d.contains('▼') || d.contains('│'), "{d}");
    }

    #[test]
    fn sequence_and_pie() {
        let seq = dump("sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi\n");
        assert!(seq.contains("Alice"), "{seq}");
        assert!(seq.contains("Bob"), "{seq}");
        assert!(seq.contains("Hello"), "{seq}");
        let pie = dump("pie title Pets\n  \"Dogs\": 40\n  \"Cats\": 30\n");
        assert!(pie.contains("Pets"), "{pie}");
        assert!(pie.contains("Dogs"), "{pie}");
        assert!(pie.contains('█'), "{pie}");
    }

    #[test]
    fn unknown_type_errors() {
        let err = render_mermaid("gantt\n  title x\n", 40).unwrap_err();
        assert!(err.contains("gantt"), "{err}");
    }
}
