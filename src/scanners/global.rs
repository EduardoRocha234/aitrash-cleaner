use crate::model::{Action, Category, Confidence, Finding};
use crate::platform::{Env, Root, APPS};
use crate::util::{age_days, aged_files, path_size};
use std::fs;
use std::path::Path;

/// Session/history dirs whose old files are safe to drop. (harness, relative path)
const AGED_DIRS: [(&str, &str); 12] = [
    ("claude", ".claude/file-history"),
    ("claude", ".claude/shell-snapshots"),
    ("claude", ".claude/todos"),
    ("claude", ".claude/debug"),
    ("claude", ".claude/paste-cache"),
    ("claude", ".claude/session-env"),
    ("codex", ".codex/sessions"),
    ("codex", ".codex/archived_sessions"),
    ("codex", ".codex/log"),
    ("gemini", ".gemini/tmp"),
    ("antigravity", ".gemini/antigravity/brain"),
    ("antigravity", ".gemini/antigravity/browser_recordings"),
];

/// Plan-mode .md files written by Claude Code.
pub const PLANS_KIND: &str = "claude: plans antigos";

/// Credential files: flagged, never removed without --hard.
const CREDENTIALS: [(&str, &str); 4] = [
    ("codex", ".codex/auth.json"),
    ("gemini", ".gemini/oauth_creds.json"),
    ("claude", ".claude/.credentials.json"),
    ("gemini", ".gemini/google_accounts.json"),
];

/// Cache subdirs inside VS Code-fork / Electron app dirs; regenerated on demand.
const APP_CACHES: [&str; 8] = ["Cache", "CachedData", "Code Cache", "GPUCache", "logs", "CachedExtensionVSIXs", "Crashpad", "DawnCache"];

pub fn scan(env: &Env, older_than: u64) -> Vec<Finding> {
    let mut out = Vec::new();
    for root in &env.roots {
        scan_root(root, older_than, &mut out);
    }
    out
}

fn aged_group(out: &mut Vec<Finding>, kind: String, dir: &Path, older_than: u64) {
    aged_group_with(out, kind, dir, older_than, Confidence::High, true);
}

fn aged_group_with(out: &mut Vec<Finding>, kind: String, dir: &Path, older_than: u64, confidence: Confidence, preselect: bool) {
    let a = aged_files(dir, older_than);
    if a.files.is_empty() {
        return;
    }
    out.push(
        Finding::new(Category::Global, kind, dir)
            .confidence(confidence)
            .size(a.bytes)
            .age(a.youngest)
            .note(format!("{} arquivo(s) com {older_than}+ dias", a.files.len()))
            .targets(a.files)
            .action(Action::Trash)
            .preselect(preselect),
    );
}

fn scan_root(root: &Root, older_than: u64, out: &mut Vec<Finding>) {
    let h = &root.home;

    // Claude transcripts: one group per project so the user can keep some.
    if let Ok(rd) = fs::read_dir(h.join(".claude/projects")) {
        for e in rd.filter_map(Result::ok).filter(|e| e.path().is_dir()) {
            aged_group(out, "claude: transcrições antigas".into(), &e.path(), older_than);
        }
    }
    for (harness, rel) in AGED_DIRS {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        aged_group(out, format!("{harness}: {name} antigos"), &h.join(rel), older_than);
    }
    // Plans may still be referenced by the user: offered, never pre-selected.
    aged_group_with(out, PLANS_KIND.into(), &h.join(".claude/plans"), older_than, Confidence::Medium, false);

    for (harness, rel) in CREDENTIALS {
        let p = h.join(rel);
        if let Ok(m) = fs::metadata(&p) {
            out.push(
                Finding::new(Category::Global, format!("{harness}: credencial"), &p)
                    .size(m.len())
                    .age(age_days(&m))
                    .note("token em texto puro; exclua de backups e dotfiles")
                    .action(Action::Trash)
                    .protect(),
            );
        }
    }

    // Big single history files: informative only.
    for (harness, rel) in [("claude", ".claude.json"), ("codex", ".codex/history.jsonl"), ("claude", ".claude/history.jsonl")] {
        let p = h.join(rel);
        if let Ok(m) = fs::metadata(&p) {
            out.push(
                Finding::new(Category::Global, format!("{harness}: histórico/config"), &p)
                    .size(m.len())
                    .age(age_days(&m))
                    .note("guarda histórico de prompts; apagar perde o estado")
                    .action(Action::Trash)
                    .protect(),
            );
        }
    }

    for app in APPS {
        for dir in root.app_dirs(app) {
            if !dir.is_dir() {
                continue;
            }
            for c in APP_CACHES {
                let p = dir.join(c);
                if p.is_dir() {
                    let size = path_size(&p);
                    if size > 0 {
                        out.push(
                            Finding::new(Category::Global, format!("{}: cache", app.to_lowercase()), &p)
                                .confidence(Confidence::High)
                                .size(size)
                                .note("regenerado automaticamente; feche o app antes")
                                .action(Action::Trash)
                                .preselect(true),
                        );
                    }
                }
            }
            out.push(
                Finding::new(Category::Global, format!("{}: dados do app", app.to_lowercase()), &dir)
                    .size(path_size(&dir))
                    .note("settings, extensões e histórico; só informativo")
                    .action(Action::None),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    fn old_file(p: &Path, days: u64) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "data").unwrap();
        let t = SystemTime::now() - Duration::from_secs(days * 86_400 + 60);
        File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
    }

    #[test]
    fn groups_old_transcripts_and_protects_credentials() {
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        old_file(&h.join(".claude/projects/-p1/a.jsonl"), 60);
        old_file(&h.join(".claude/projects/-p1/b.jsonl"), 1);
        old_file(&h.join(".codex/sessions/2025/01/01/r.jsonl"), 90);
        old_file(&h.join(".codex/auth.json"), 1);
        old_file(&h.join(".claude/plans/old-plan.md"), 45);
        old_file(&h.join(".claude/plans/new-plan.md"), 2);
        fs::create_dir_all(h.join(".config/Cursor/CachedData")).unwrap();
        fs::write(h.join(".config/Cursor/CachedData/x"), "1234").unwrap();

        let env = Env::single(h.to_path_buf(), Os::Linux);
        let f = scan(&env, 30);

        let claude = f.iter().find(|f| f.kind.starts_with("claude: transcrições")).unwrap();
        assert_eq!(claude.targets.len(), 1);
        assert!(claude.targets[0].ends_with("a.jsonl"));
        assert!(claude.preselected);

        assert!(f.iter().any(|f| f.kind == "codex: sessions antigos" && f.targets.len() == 1));

        let plans = f.iter().find(|f| f.kind == PLANS_KIND).unwrap();
        assert_eq!(plans.targets.len(), 1);
        assert!(plans.targets[0].ends_with("old-plan.md"));
        assert!(!plans.preselected);

        let auth = f.iter().find(|f| f.path.ends_with("auth.json")).unwrap();
        assert!(auth.protected && !auth.preselected);

        let cache = f.iter().find(|f| f.kind == "cursor: cache").unwrap();
        assert_eq!(cache.size, Some(4));
        assert!(f.iter().any(|f| f.kind == "cursor: dados do app" && f.action == Action::None));
    }
}
