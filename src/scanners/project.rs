use crate::platform::DOT_DIRS;
use crate::model::{Action, Category, Confidence, Finding};
use crate::rules::{classify, SKIP_DIRS};
use crate::util::{age_days, path_size};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

pub struct ProjectScan {
    pub findings: Vec<Finding>,
    /// Git repositories found while walking (used by the system scanner).
    pub repos: Vec<PathBuf>,
}

struct Repo {
    root: PathBuf,
    tracked: HashSet<PathBuf>,
}

fn tracked_files(root: &Path) -> HashSet<PathBuf> {
    let Ok(out) = Command::new("git").arg("-C").arg(root).args(["ls-files", "-z"]).output() else {
        return HashSet::new();
    };
    out.stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| root.join(String::from_utf8_lossy(s).as_ref()))
        .collect()
}

/// True if `p` (file or dir) is tracked, or a dir containing tracked files.
fn is_tracked(repo: &Repo, p: &Path, is_dir: bool) -> bool {
    if is_dir {
        repo.tracked.iter().any(|t| t.starts_with(p))
    } else {
        repo.tracked.contains(p)
    }
}

pub fn scan(paths: &[PathBuf], home: &Path) -> ProjectScan {
    let mut findings = Vec::new();
    let mut repos: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();

    for base in paths {
        // Stack of enclosing repos (walk is depth-first).
        let mut stack: Vec<Repo> = Vec::new();
        let mut it = WalkDir::new(base).follow_links(false).into_iter();
        while let Some(entry) = it.next() {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let is_dir = entry.file_type().is_dir();
            let name = entry.file_name().to_string_lossy();

            while stack.last().is_some_and(|r| !path.starts_with(&r.root)) {
                stack.pop();
            }

            if is_dir {
                // The home's own agent dirs belong to the global scanner.
                let home_dot = path.parent() == Some(home) && DOT_DIRS.contains(&name.as_ref());
                if entry.depth() > 0 && (home_dot || SKIP_DIRS.contains(&name.as_ref())) {
                    it.skip_current_dir();
                    continue;
                }
                if path.join(".git").exists() && seen.insert(path.to_path_buf()) {
                    repos.push(path.to_path_buf());
                    stack.push(Repo { root: path.to_path_buf(), tracked: tracked_files(path) });
                }
            }

            let Some(rule) = classify(&name, is_dir) else { continue };
            if is_dir {
                it.skip_current_dir();
            }
            // Agent config dirs only matter inside a project, not the home itself.
            if entry.depth() == 0 {
                continue;
            }

            let meta = entry.metadata().ok();
            let size = if is_dir { path_size(path) } else { meta.as_ref().map_or(0, |m| m.len()) };
            let mut conf = rule.confidence;
            let mut tracked = false;
            let mut note = String::new();
            if let Some(repo) = stack.last() {
                tracked = is_tracked(repo, path, is_dir);
                if tracked {
                    note = "rastreado pelo git".into();
                } else if conf != Confidence::Info {
                    conf = conf.bump();
                    note = "não rastreado pelo git".into();
                }
            }

            let mut f = Finding::new(Category::Project, rule.kind, path)
                .confidence(conf)
                .size(size)
                .age(meta.as_ref().and_then(age_days))
                .note(note)
                .action(Action::Trash)
                .preselect(conf == Confidence::High && !tracked);
            if tracked {
                f = f.protect();
            }
            findings.push(f);
        }
    }
    ProjectScan { findings, repos }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn finds_junk_and_respects_git() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        git(r, &["init", "-q"]);
        fs::write(r.join("main.py"), "print()").unwrap();
        fs::write(r.join("AGENTS.md"), "x").unwrap();
        fs::write(r.join("old.py.bak"), "x").unwrap();
        fs::write(r.join("test_fix_auth.py"), "x").unwrap();
        fs::write(r.join("debug_tracked.js"), "x").unwrap();
        fs::create_dir_all(r.join(".claude")).unwrap();
        fs::write(r.join(".claude/settings.local.json"), "{}").unwrap();
        fs::create_dir_all(r.join("node_modules/x")).unwrap();
        fs::write(r.join("node_modules/x/a.bak"), "x").unwrap();
        git(r, &["add", "main.py", "AGENTS.md", "debug_tracked.js"]);
        git(r, &["commit", "-qm", "init"]);

        let s = scan(&[r.to_path_buf()], Path::new("/nonexistent"));
        assert_eq!(s.repos, vec![r.to_path_buf()]);
        let get = |n: &str| s.findings.iter().find(|f| f.path.ends_with(n)).unwrap_or_else(|| panic!("{n}"));

        let bak = get("old.py.bak");
        assert!(bak.preselected && !bak.protected);

        // untracked medium bumps to high
        let tf = get("test_fix_auth.py");
        assert_eq!(tf.confidence, Confidence::High);
        assert!(tf.preselected);

        // tracked: protected, never preselected
        let dbg = get("debug_tracked.js");
        assert!(dbg.protected && !dbg.preselected);

        let cfg = get(".claude");
        assert_eq!(cfg.confidence, Confidence::Info);
        assert!(!cfg.preselected);

        assert!(get("AGENTS.md").protected);
        assert!(!s.findings.iter().any(|f| f.path.to_string_lossy().contains("node_modules")));
        assert!(!s.findings.iter().any(|f| f.path.ends_with("main.py")));
    }

    #[test]
    fn no_git_no_bump() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("check_x.sh"), "x").unwrap();
        let s = scan(&[d.path().to_path_buf()], Path::new("/nonexistent"));
        assert_eq!(s.findings.len(), 1);
        assert_eq!(s.findings[0].confidence, Confidence::Medium);
        assert!(!s.findings[0].preselected);
    }

    #[test]
    fn home_agent_dirs_are_skipped() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join(".claude")).unwrap();
        fs::write(d.path().join(".claude/x.bak"), "x").unwrap();
        fs::create_dir_all(d.path().join("proj/.claude")).unwrap();
        let s = scan(&[d.path().to_path_buf()], d.path());
        assert_eq!(s.findings.len(), 1);
        assert!(s.findings[0].path.ends_with("proj/.claude"));
    }
}
