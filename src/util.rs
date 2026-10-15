use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Age in whole days since last modification.
pub fn age_days(meta: &fs::Metadata) -> Option<u64> {
    let m = meta.modified().ok()?;
    let d = SystemTime::now().duration_since(m).ok()?;
    Some(d.as_secs() / 86_400)
}

/// Total size of a file or directory tree. Never follows symlinks.
pub fn path_size(p: &Path) -> u64 {
    WalkDir::new(p)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Sizes of many trees at once. Work is split per top-level child across a thread pool,
/// which matters on slow filesystems (WSL `/mnt/c`, network drives).
pub fn sizes(paths: &[PathBuf]) -> Vec<u64> {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    // (owner index, path to walk)
    let mut jobs: Vec<(usize, PathBuf)> = Vec::new();
    let totals: Vec<AtomicU64> = paths.iter().map(|_| AtomicU64::new(0)).collect();
    for (i, p) in paths.iter().enumerate() {
        match fs::read_dir(p) {
            Ok(rd) if fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) => {
                jobs.extend(rd.filter_map(Result::ok).map(|e| (i, e.path())))
            }
            _ => jobs.push((i, p.clone())),
        }
    }
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get() * 2).min(jobs.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let j = next.fetch_add(1, Ordering::Relaxed);
                let Some((i, p)) = jobs.get(j) else { break };
                totals[*i].fetch_add(path_size(p), Ordering::Relaxed);
            });
        }
    });
    totals.into_iter().map(AtomicU64::into_inner).collect()
}

/// Files under `dir` not modified for at least `min_age` days.
/// Returns (files, total bytes, youngest age).
pub struct Aged {
    pub files: Vec<PathBuf>,
    pub bytes: u64,
    pub youngest: Option<u64>,
}

pub fn aged_files(dir: &Path, min_age: u64) -> Aged {
    let mut out = Aged {
        files: Vec::new(),
        bytes: 0,
        youngest: None,
    };
    for e in WalkDir::new(dir).follow_links(false).into_iter().filter_map(Result::ok) {
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() {
            continue;
        }
        let Some(age) = age_days(&m) else { continue };
        if age >= min_age {
            out.bytes += m.len();
            out.youngest = Some(out.youngest.map_or(age, |y: u64| y.min(age)));
            out.files.push(e.into_path());
        }
    }
    out
}

/// Parses "30", "30d", "2w", "6m" (months = 30d), "1y" into days.
pub fn parse_days(s: &str) -> Result<u64, String> {
    let s = s.trim().to_ascii_lowercase();
    let (num, mult) = match s.chars().last() {
        Some('d') => (&s[..s.len() - 1], 1),
        Some('w') => (&s[..s.len() - 1], 7),
        Some('m') => (&s[..s.len() - 1], 30),
        Some('y') => (&s[..s.len() - 1], 365),
        _ => (s.as_str(), 1),
    };
    num.parse::<u64>()
        .map(|n| n * mult)
        .map_err(|_| format!("duração inválida: '{s}' (use ex.: 30d, 2w, 6m)"))
}

/// Replaces the home prefix by `~` for display.
pub fn pretty_path(p: &Path, home: &Path) -> String {
    match p.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        Err(_) => p.display().to_string(),
    }
}

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|d| {
        let p = d.join(bin);
        if p.is_file() {
            return Some(p);
        }
        let exe = d.join(format!("{bin}.exe"));
        exe.is_file().then_some(exe)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn days() {
        assert_eq!(parse_days("30").unwrap(), 30);
        assert_eq!(parse_days("30d").unwrap(), 30);
        assert_eq!(parse_days("2w").unwrap(), 14);
        assert_eq!(parse_days("1y").unwrap(), 365);
        assert!(parse_days("abc").is_err());
    }

    #[test]
    fn pretty() {
        let h = Path::new("/home/u");
        assert_eq!(pretty_path(Path::new("/home/u/.claude"), h), "~/.claude");
        assert_eq!(pretty_path(Path::new("/tmp/x"), h), "/tmp/x");
    }

    #[test]
    fn aged_and_size() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a"), b"hello").unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/b"), b"abc").unwrap();
        assert_eq!(path_size(d.path()), 8);
        assert_eq!(sizes(&[d.path().to_path_buf(), d.path().join("sub"), d.path().join("missing")]), vec![8, 3, 0]);
        let a = aged_files(d.path(), 0);
        assert_eq!(a.files.len(), 2);
        assert_eq!(a.bytes, 8);
        assert_eq!(aged_files(d.path(), 1).files.len(), 0);
    }
}
