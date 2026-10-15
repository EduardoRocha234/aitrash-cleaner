use crate::model::{Action, Category, Confidence, Finding};
use crate::platform::Env;
use crate::rules::{mask, plausible, SECRET_PATTERNS, SECRET_SET};
use crate::util::age_days;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Where agents keep transcripts, file snapshots and config that may echo secrets.
const SECRET_DIRS: [&str; 8] = [
    ".claude/projects",
    ".claude/file-history",
    ".claude/shell-snapshots",
    ".claude/history.jsonl",
    ".claude.json",
    ".codex",
    ".gemini",
    ".cursor/mcp.json",
];

const MAX_FILE: u64 = 512 * 1024 * 1024;

pub struct Hit {
    pub line: usize,
    pub kind: &'static str,
    pub masked: String,
}

fn is_binary(p: &Path) -> bool {
    let mut buf = [0u8; 4096];
    match File::open(p).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => buf[..n].contains(&0),
        Err(_) => true,
    }
}

/// Scans one file line by line. Each distinct secret is reported once.
pub fn scan_file(p: &Path) -> Vec<Hit> {
    let Ok(f) = File::open(p) else { return Vec::new() };
    let mut reader = BufReader::new(f);
    let mut seen = std::collections::HashSet::new();
    let mut hits = Vec::new();
    let mut line = Vec::new();
    let mut n = 0;
    while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
        n += 1;
        let matched = SECRET_SET.matches(&line);
        for (i, pat) in SECRET_PATTERNS.iter().enumerate() {
            if !matched.matched(i) {
                continue;
            }
            for caps in pat.re.captures_iter(&line) {
                let Some(m) = caps.get(pat.group) else { continue };
                let v = String::from_utf8_lossy(m.as_bytes());
                if pat.name == "openai key" && v.starts_with("sk-ant-") {
                    continue;
                }
                let var = (pat.group > 1).then(|| String::from_utf8_lossy(&caps[1]).into_owned());
                if !plausible(pat.name, var.as_deref(), &v) || !seen.insert(v.to_string()) {
                    continue;
                }
                let masked = match var {
                    Some(var) => format!("{var}={}", mask(&v)),
                    None => mask(&v),
                };
                hits.push(Hit { line: n, kind: pat.name, masked });
            }
        }
        line.clear();
    }
    hits
}

/// Files that hold config the user needs (rc files, `.claude.json`): report, never auto-remove.
const KEEP: [&str; 2] = [".claude.json", ".cursor/mcp.json"];

/// (file, protected)
fn candidate_files(env: &Env) -> Vec<(PathBuf, bool)> {
    let mut files = Vec::new();
    for root in &env.roots {
        let mut starts: Vec<(PathBuf, bool)> =
            SECRET_DIRS.iter().map(|r| (root.home.join(r), KEEP.contains(r))).collect();
        starts.extend(root.shell_rc_files().into_iter().map(|p| (p, true)));
        for (s, protected) in starts {
            for e in WalkDir::new(&s).follow_links(false).into_iter().filter_map(Result::ok) {
                let ok = e.file_type().is_file() && e.metadata().is_ok_and(|m| m.len() <= MAX_FILE);
                if ok && !is_binary(e.path()) {
                    files.push((e.into_path(), protected));
                }
            }
        }
    }
    files
}

fn scan_parallel(files: &[(PathBuf, bool)]) -> Vec<Vec<Hit>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let results: Vec<Mutex<Vec<Hit>>> = files.iter().map(|_| Mutex::new(Vec::new())).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((p, _)) = files.get(i) else { break };
                *results[i].lock().unwrap() = scan_file(p);
            });
        }
    });
    results.into_iter().map(|m| m.into_inner().unwrap()).collect()
}

/// `max_details`: how many masked hits to keep per file (None = all).
pub fn scan(env: &Env, max_details: Option<usize>) -> Vec<Finding> {
    let files = candidate_files(env);
    let mut out = Vec::new();
    for ((p, protected), hits) in files.iter().zip(scan_parallel(&files)) {
        if hits.is_empty() {
            continue;
        }
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for h in &hits {
            *by_kind.entry(h.kind).or_default() += 1;
        }
        let summary = by_kind.iter().map(|(k, n)| format!("{k} ×{n}")).collect::<Vec<_>>().join(", ");
        let details = hits
            .iter()
            .take(max_details.unwrap_or(usize::MAX))
            .map(|h| format!("linha {}: {} {}", h.line, h.kind, h.masked))
            .collect();
        let meta = std::fs::metadata(p).ok();
        let mut f = Finding::new(Category::Secrets, summary, p)
            .confidence(Confidence::High)
            .size(meta.as_ref().map_or(0, |m| m.len()))
            .age(meta.as_ref().and_then(age_days))
            .note("rotacione as chaves; apagar o arquivo remove a cópia local")
            .details(details)
            .action(Action::Trash);
        if *protected {
            f = f.protect();
        }
        out.push(f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;
    use std::fs;

    #[test]
    fn finds_and_masks_secrets() {
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        let dir = h.join(".claude/projects/-p");
        fs::create_dir_all(&dir).unwrap();
        let ant = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFF0000";
        let gh = "ghp_abcdefghijklmnopqrstuvwxyz0123456789AB";
        let content = format!(
            "{{\"text\":\"key {ant}\"}}\n{{\"text\":\"again {ant}\"}}\n{{\"out\":\"DB_PASSWORD=hunter2hunter2\\nAPI_KEY=your_key_here\"}}\n{{\"t\":\"{gh}\"}}\n"
        );
        fs::write(dir.join("s.jsonl"), content).unwrap();
        fs::write(dir.join("clean.jsonl"), "{\"text\":\"nothing here\"}\n").unwrap();

        let env = Env::single(h.to_path_buf(), Os::Linux);
        let f = scan(&env, None);
        assert_eq!(f.len(), 1);
        let s = &f[0];
        assert!(s.kind.contains("anthropic key ×1"), "{}", s.kind);
        assert!(s.kind.contains("github token ×1"));
        assert!(s.kind.contains(".env secret ×1"));
        assert!(!s.kind.contains("openai"));
        let all = format!("{:?}{}", s.details, serde_json::to_string(s).unwrap());
        assert!(!all.contains(ant) && !all.contains(gh) && !all.contains("hunter2hunter2"));
        assert!(all.contains("DB_PASSWORD=hu…"));
    }

    #[test]
    fn rc_files_are_protected() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(".bashrc"), "export ANTHROPIC_API_KEY=sk-ant-api03-ZZZZYYYYQQQQWWWWVVVV1111\n").unwrap();
        let env = Env::single(d.path().to_path_buf(), Os::Linux);
        let f = scan(&env, None);
        assert_eq!(f.len(), 1);
        assert!(f[0].protected);
    }
}
