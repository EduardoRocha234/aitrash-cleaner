use crate::model::{Action, Category, Confidence, Finding};
use crate::platform::Env;
use crate::util::{sizes, which};
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Command-line fragments of dev servers, watchers and MCP servers agents tend to leave running.
const PROC_PATTERNS: [&str; 17] = [
    "vite", "next dev", "next-server", "npm run dev", "pnpm dev", "yarn dev", "uvicorn", "flask run",
    "webpack serve", "nodemon", "tsc --watch", "tsc -w", "mcp", "--remote-debugging-port", "playwright",
    "http.server", "modelcontextprotocol",
];

/// Branch prefixes created by agents / their worktree features.
const AGENT_BRANCHES: [&str; 6] = ["claude/*", "codex/*", "cursor/*", "copilot/*", "agent/*", "worktree-*"];

/// Agent-related markers looked for in shell rc files.
const RC_MARKERS: [&str; 8] = ["claude", "codex", "gemini", "cursor", "windsurf", "antigravity", "anthropic", "openai"];

pub fn scan(env: &Env, repos: &[PathBuf]) -> Vec<Finding> {
    let mut out = Vec::new();
    caches(env, &mut out);
    processes(&mut out);
    for r in repos {
        git(r, &mut out);
    }
    docker(&mut out);
    shell_rc(env, &mut out);
    out
}

fn caches(env: &Env, out: &mut Vec<Finding>) {
    for root in env.roots.iter().filter(|r| r.slow) {
        for (label, p, cmd) in root.caches().into_iter().filter(|(_, p, _)| p.is_dir()) {
            out.push(
                Finding::new(Category::System, format!("cache: {label}"), &p)
                    .confidence(Confidence::Medium)
                    .note(format!("tamanho omitido (disco lento via WSL; veja `aitrash du`). comando oficial: {cmd}"))
                    .action(Action::Trash),
            );
        }
    }
    let found: Vec<_> = env
        .roots
        .iter()
        .filter(|r| !r.slow)
        .flat_map(|r| r.caches())
        .filter(|(_, p, _)| p.is_dir())
        .collect();
    let paths: Vec<PathBuf> = found.iter().map(|(_, p, _)| p.clone()).collect();
    for ((label, p, cmd), size) in found.into_iter().zip(sizes(&paths)) {
        if size == 0 {
            continue;
        }
        out.push(
            Finding::new(Category::System, format!("cache: {label}"), &p)
                .confidence(Confidence::Medium)
                .size(size)
                .note(format!("comando oficial: {cmd}"))
                .action(Action::Trash),
        );
    }
}

pub fn matches_dev_process(cmdline: &str) -> bool {
    let l = cmdline.to_ascii_lowercase();
    PROC_PATTERNS.iter().any(|p| l.contains(p))
}

fn processes(out: &mut Vec<Finding>) {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_cwd(UpdateKind::Always),
    );
    let me = sysinfo::get_current_pid().ok();
    for (pid, p) in sys.processes() {
        // Skip threads and ourselves.
        if Some(*pid) == me || p.thread_kind().is_some() {
            continue;
        }
        let cmd = p.cmd().iter().map(|s| s.to_string_lossy()).collect::<Vec<_>>().join(" ");
        if cmd.is_empty() || !matches_dev_process(&cmd) {
            continue;
        }
        let parent = p.parent();
        let orphan = match parent {
            None => true,
            Some(pp) => pp.as_u32() <= 1 || sys.process(pp).is_none_or(|pp| {
                let n = pp.name().to_string_lossy().to_ascii_lowercase();
                n == "init" || n == "systemd" || n == "launchd"
            }),
        };
        // A dev server with a live parent (terminal, agent session) is intentional.
        if !orphan {
            continue;
        }
        let short: String = cmd.chars().take(90).collect();
        let cwd = p.cwd().map(Path::to_path_buf).unwrap_or_default();
        out.push(
            Finding::new(Category::System, format!("processo órfão pid {pid}"), cwd)
                .confidence(Confidence::Medium)
                .note(short)
                .action(Action::KillPid(pid.as_u32())),
        );
    }
}

fn git_out(repo: &Path, args: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn git(repo: &Path, out: &mut Vec<Finding>) {
    // Extra worktrees (first entry is the main one).
    let wt = git_out(repo, &["worktree", "list", "--porcelain"]);
    for block in wt.split("\n\n").skip(1) {
        let Some(path) = block.lines().find_map(|l| l.strip_prefix("worktree ")) else { continue };
        let prunable = block.lines().any(|l| l.starts_with("prunable"));
        out.push(
            Finding::new(Category::System, "git: worktree extra", repo)
                .confidence(if prunable { Confidence::High } else { Confidence::Low })
                .note(if prunable { "worktree órfã (diretório sumiu)".to_string() } else { path.to_string() })
                .action(Action::Suggest(if prunable {
                    format!("git -C '{}' worktree prune", repo.display())
                } else {
                    format!("git -C '{}' worktree remove '{path}'", repo.display())
                })),
        );
    }

    let stashes = git_out(repo, &["stash", "list"]).lines().count();
    if stashes > 0 {
        out.push(
            Finding::new(Category::System, "git: stashes", repo)
                .confidence(Confidence::Low)
                .note(format!("{stashes} stash(es) esquecido(s)"))
                .action(Action::Suggest(format!("git -C '{}' stash list  # depois: git stash drop", repo.display()))),
        );
    }

    let mut args = vec!["branch", "--list", "--format=%(refname:short)"];
    args.extend(AGENT_BRANCHES);
    let branches: Vec<String> = git_out(repo, &args).lines().map(str::to_string).collect();
    if !branches.is_empty() {
        out.push(
            Finding::new(Category::System, "git: branches de agente", repo)
                .confidence(Confidence::Low)
                .note(format!("{} branch(es): {}", branches.len(), branches.join(", ")))
                .action(Action::Suggest(format!("git -C '{}' branch -D {}", repo.display(), branches.join(" ")))),
        );
    }
}

fn docker(out: &mut Vec<Finding>) {
    if which("docker").is_none() {
        return;
    }
    let count = |args: &[&str]| -> Option<usize> {
        let o = Command::new("docker").args(args).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).lines().count())
    };
    if let Some(n) = count(&["ps", "-aq", "--filter", "status=exited"]).filter(|n| *n > 0) {
        out.push(
            Finding::new(Category::System, "docker: containers parados", "docker")
                .confidence(Confidence::Low)
                .note(format!("{n} container(s)"))
                .action(Action::Suggest("docker container prune".into())),
        );
    }
    if let Some(n) = count(&["images", "-qf", "dangling=true"]).filter(|n| *n > 0) {
        out.push(
            Finding::new(Category::System, "docker: imagens órfãs", "docker")
                .confidence(Confidence::Medium)
                .note(format!("{n} imagem(ns) dangling"))
                .action(Action::Suggest("docker image prune".into())),
        );
    }
}

fn shell_rc(env: &Env, out: &mut Vec<Finding>) {
    for root in &env.roots {
        for rc in root.shell_rc_files() {
            let Ok(text) = std::fs::read_to_string(&rc) else { continue };
            let lines: Vec<String> = text
                .lines()
                .enumerate()
                .filter(|(_, l)| {
                    let l = l.to_ascii_lowercase();
                    !l.trim_start().starts_with('#') && RC_MARKERS.iter().any(|m| l.contains(m))
                })
                // Values may be secrets: show only the variable/command name.
                .map(|(i, l)| {
                    let head: String = l.split('=').next().unwrap_or(l).chars().take(60).collect();
                    format!("linha {}: {}", i + 1, head.trim())
                })
                .collect();
            if !lines.is_empty() {
                out.push(
                    Finding::new(Category::System, "shell rc alterado", &rc)
                        .note(format!("{} linha(s) ligadas a agentes de IA", lines.len()))
                        .details(lines)
                        .action(Action::None),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Os;
    use std::fs;

    #[test]
    fn dev_process_patterns() {
        assert!(matches_dev_process("node /x/node_modules/.bin/vite --port 5173"));
        assert!(matches_dev_process("npx -y @modelcontextprotocol/server-filesystem"));
        assert!(!matches_dev_process("/usr/bin/bash"));
    }

    #[test]
    fn caches_and_rc() {
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        fs::create_dir_all(h.join(".cache/ms-playwright/chromium")).unwrap();
        fs::write(h.join(".cache/ms-playwright/chromium/bin"), vec![0u8; 100]).unwrap();
        fs::write(h.join(".bashrc"), "# claude comment\nexport PATH=$PATH:~/.claude/bin\nexport OPENAI_API_KEY=sk-secret\nalias ll=ls\n").unwrap();
        let env = Env::single(h.to_path_buf(), Os::Linux);
        let mut out = Vec::new();
        caches(&env, &mut out);
        shell_rc(&env, &mut out);
        let c = out.iter().find(|f| f.kind == "cache: playwright browsers").unwrap();
        assert_eq!(c.size, Some(100));
        assert!(!c.preselected);
        let rc = out.iter().find(|f| f.kind == "shell rc alterado").unwrap();
        assert_eq!(rc.details.len(), 2);
        assert!(!rc.details.join("").contains("sk-secret"));
    }

    #[test]
    fn git_agent_branches_and_stash() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let g = |a: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(r).args(["-c", "user.email=t@t", "-c", "user.name=t"]).args(a).status().unwrap().success())
        };
        g(&["init", "-q"]);
        fs::write(r.join("a"), "1").unwrap();
        g(&["add", "a"]);
        g(&["commit", "-qm", "i"]);
        g(&["branch", "claude/fix-x"]);
        fs::write(r.join("a"), "2").unwrap();
        g(&["stash", "-q"]);
        let mut out = Vec::new();
        git(r, &mut out);
        assert!(out.iter().any(|f| f.kind == "git: branches de agente" && f.note.contains("claude/fix-x")));
        assert!(out.iter().any(|f| f.kind == "git: stashes"));
    }
}
