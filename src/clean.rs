use crate::groups::Group;
use crate::model::{Action, Finding};
use crate::report::label;
use crate::util::{human_size, pretty_path};
use anstream::{eprint, eprintln, println};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use anstyle::{AnsiColor, Style};
use dialoguer::console::{self, style};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, MultiSelect};
use std::fs;
use std::path::{Path, PathBuf};

const GREEN: Style = AnsiColor::Green.on_default().bold();
const RED: Style = AnsiColor::Red.on_default().bold();
const DIM: Style = Style::new().dimmed();

pub struct CleanOpts {
    pub yes: bool,
    pub hard: bool,
    pub dry_run: bool,
    pub interactive: bool,
    pub verbose: bool,
    /// Removal is only allowed strictly inside one of these (absolute) roots.
    pub allowed_roots: Vec<PathBuf>,
}

/// True if `p` lies strictly inside an allowed root (never a root itself), after
/// resolving `..` and symlinks in its parent. The entry itself is not resolved,
/// so a symlink is removed as a link, never followed.
pub fn is_allowed(p: &Path, roots: &[PathBuf]) -> bool {
    let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else { return false };
    let Ok(parent) = fs::canonicalize(parent) else { return false };
    let full = parent.join(name);
    roots.iter().any(|r| {
        let r = fs::canonicalize(r).unwrap_or_else(|_| r.clone());
        full.starts_with(&r) && full != r
    })
}

fn remove(p: &Path, hard: bool) -> Result<(), String> {
    let meta = fs::symlink_metadata(p).map_err(|e| e.to_string())?;
    if hard {
        if meta.is_dir() { remove_dir_parallel(p) } else { fs::remove_file(p) }.map_err(|e| e.to_string())
    } else {
        trash::delete(p).map_err(|e| e.to_string())
    }
}

/// `remove_dir_all` deletes one file at a time; on huge caches (and on WSL's slow `/mnt/c`)
/// that takes minutes. Split the tree into subtrees and delete them on a thread pool.
/// Never follows symlinks: a symlinked dir is removed as a link (`remove_dir_all` is symlink-safe).
fn remove_dir_parallel(root: &Path) -> std::io::Result<()> {
    const TARGET_JOBS: usize = 256;
    let mut jobs: Vec<PathBuf> = vec![root.to_path_buf()];
    // Expand directories breadth-first until there is enough parallel work.
    for _ in 0..3 {
        if jobs.len() >= TARGET_JOBS {
            break;
        }
        let mut next = Vec::new();
        for j in jobs {
            match fs::symlink_metadata(&j) {
                Ok(m) if m.is_dir() => match fs::read_dir(&j) {
                    Ok(rd) => next.extend(rd.filter_map(Result::ok).map(|e| e.path())),
                    Err(_) => next.push(j),
                },
                _ => next.push(j),
            }
        }
        jobs = next;
    }

    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get() * 4).min(jobs.len().max(1));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(p) = jobs.get(i) else { break };
                let _ = match fs::symlink_metadata(p) {
                    Ok(m) if m.is_dir() => fs::remove_dir_all(p),
                    Ok(_) => fs::remove_file(p),
                    Err(_) => Ok(()),
                };
            });
        }
    });
    // Leftovers (now-empty dirs, anything a worker failed on) and the root itself;
    // reports the real error if something could not be removed.
    if root.symlink_metadata().is_ok() { fs::remove_dir_all(root) } else { Ok(()) }
}

fn kill(pid: u32) -> bool {
    let mut sys = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).is_some_and(|p| p.kill())
}

fn print_suggestions(findings: &[Finding]) {
    let cmds: Vec<&String> = findings
        .iter()
        .filter_map(|f| if let Action::Suggest(c) = &f.action { Some(c) } else { None })
        .collect();
    if !cmds.is_empty() {
        println!("\n{DIM}Comandos sugeridos (não executados automaticamente):{DIM:#}");
        for c in cmds {
            println!("  $ {c}");
        }
    }
}

/// Red accent theme for every interactive prompt (dialoguer draws on stderr).
fn theme() -> ColorfulTheme {
    let st = |s: &str| style(s.to_string()).for_stderr();
    ColorfulTheme {
        prompt_prefix: st("?").red().bold(),
        prompt_style: console::Style::new().for_stderr().bold(),
        success_prefix: st("✔").red().bold(),
        values_style: console::Style::new().for_stderr().red(),
        active_item_style: console::Style::new().for_stderr().red().bold(),
        active_item_prefix: st("❯").red().bold(),
        checked_item_prefix: st("◉").red().bold(),
        unchecked_item_prefix: st("◯").dim(),
        hint_style: console::Style::new().for_stderr().dim(),
        ..ColorfulTheme::default()
    }
}

/// Menu row: label and size plain (so the active-row style applies), description dimmed.
fn menu_item(g: &Group) -> String {
    let extra = if g.key.starts_with("cache:") { "; baixa de novo se precisar" } else { "" };
    let desc = style(format!("{}{extra}", g.description())).for_stderr().dim();
    format!("{:<28}{:>10}   {desc}", g.label, g.size_label())
}

fn pick(cands: &[&Finding], home: &Path) -> Vec<usize> {
    let items: Vec<String> = cands.iter().map(|f| label(f, home)).collect();
    let defaults: Vec<bool> = cands.iter().map(|f| f.preselected).collect();
    MultiSelect::with_theme(&theme())
        .with_prompt("Espaço marca/desmarca, Enter confirma, Esc cancela")
        .report(false)
        .items(&items)
        .defaults(&defaults)
        .max_length(20)
        .interact_opt()
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn confirm(prompt: String) -> bool {
    Confirm::with_theme(&theme()).with_prompt(prompt).default(false).interact().unwrap_or(false)
}

struct Outcome {
    ok: usize,
    freed: u64,
    failed: Vec<(PathBuf, String)>,
}

/// Removes/kills `chosen`. Protected items must already have been confirmed.
fn execute(chosen: &[&Finding], o: &CleanOpts) -> Outcome {
    let mut out = Outcome { ok: 0, freed: 0, failed: Vec::new() };
    for f in chosen {
        match f.action {
            Action::KillPid(pid) => {
                if kill(pid) {
                    out.ok += 1;
                } else {
                    out.failed.push((f.path.clone(), format!("não consegui matar pid {pid}")));
                }
            }
            Action::Trash => {
                let mut all_ok = true;
                for t in f.removal_targets() {
                    if !is_allowed(&t, &o.allowed_roots) {
                        out.failed.push((t, "fora das pastas permitidas; ignorado".into()));
                        all_ok = false;
                        continue;
                    }
                    if let Err(e) = remove(&t, o.hard) {
                        out.failed.push((t, e));
                        all_ok = false;
                    }
                }
                if all_ok {
                    out.ok += 1;
                    out.freed += f.size.unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    out
}

/// Trash can fail across filesystems (e.g. /mnt/c under WSL): offer a hard delete.
fn retry_hard(out: &mut Outcome, o: &CleanOpts) {
    if !o.hard && !out.failed.is_empty() && o.interactive {
        let retry: Vec<PathBuf> = out
            .failed
            .iter()
            .filter(|(p, _)| is_allowed(p, &o.allowed_roots) && p.symlink_metadata().is_ok())
            .map(|(p, _)| p.clone())
            .collect();
        if !retry.is_empty() && confirm(format!("Lixeira falhou para {} caminho(s). Apagar permanentemente?", retry.len())) {
            out.failed.retain(|(p, _)| !retry.contains(p));
            for p in retry {
                if let Err(e) = remove(&p, true) {
                    out.failed.push((p, e));
                }
            }
        }
    }
}

/// Animated "working" line on stderr, so long deletions don't look frozen.
struct Spinner {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Spinner {
    fn start(msg: String, enabled: bool) -> Spinner {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = enabled.then(|| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                let t0 = Instant::now();
                let mut i = 0;
                while !stop.load(Ordering::Relaxed) {
                    eprint!("\r\x1b[2K{RED}{}{RED:#} {msg} {DIM}{}s{DIM:#}", FRAMES[i % FRAMES.len()], t0.elapsed().as_secs());
                    i += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
                eprint!("\r\x1b[2K");
            })
        });
        Spinner { stop, handle }
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Runs each batch (label, size label, items) with a spinner and a ✔/✗ line when it ends.
fn execute_batches(batches: &[(String, String, Vec<&Finding>)], o: &CleanOpts) -> Outcome {
    let verb = if o.hard { "apagando" } else { "movendo para a lixeira" };
    let mut total = Outcome { ok: 0, freed: 0, failed: Vec::new() };
    for (i, (label, size, items)) in batches.iter().enumerate() {
        let started = Instant::now();
        let sp = Spinner::start(format!("{verb} {label} ({}/{})", i + 1, batches.len()), o.interactive);
        let out = execute(items, o);
        sp.finish();
        let secs = started.elapsed().as_secs();
        if out.failed.is_empty() {
            println!("  {RED}✔{RED:#} {label:<28}{size:>10}  {DIM}{secs}s{DIM:#}");
        } else {
            println!("  {RED}✗{RED:#} {label:<28}{size:>10}  {DIM}{} erro(s){DIM:#}", out.failed.len());
        }
        total.ok += out.ok;
        total.freed += out.freed;
        total.failed.extend(out.failed);
    }
    retry_hard(&mut total, o);
    total
}

fn report_outcome(out: &Outcome, what: &str, home: &Path) {
    println!("{GREEN}✓ {} {what} limpo(s), ~{} liberado(s).{GREEN:#}", out.ok, human_size(out.freed));
    for (p, e) in out.failed.iter().take(5) {
        eprintln!("{RED}✗{RED:#} {}: {e}", pretty_path(p, home));
    }
    if out.failed.len() > 5 {
        eprintln!("{RED}✗{RED:#} … +{} erro(s)", out.failed.len() - 5);
    }
}

fn confirm_protected(f: &Finding, home: &Path, o: &CleanOpts) -> bool {
    !f.protected || (o.interactive && confirm(format!("ITEM PROTEGIDO: remover {}?", pretty_path(&f.path, home))))
}

fn final_confirm(o: &CleanOpts, what: String, bytes: u64) -> bool {
    let verb = if o.hard { "APAGAR PERMANENTEMENTE" } else { "Mover para a lixeira" };
    o.yes || confirm(format!("{verb} {what} ({})?", human_size(bytes)))
}

/// Default `clean`: one checkbox per group.
pub fn run_groups(groups: &[Group], findings: &[Finding], home: &Path, o: &CleanOpts) {
    if groups.is_empty() {
        println!("{GREEN}Nada para limpar.{GREEN:#}");
        return;
    }
    if o.dry_run || (!o.interactive && !o.yes) {
        if !o.dry_run {
            println!("{DIM}Sem terminal interativo: dry-run. Use --yes para aplicar os marcados.{DIM:#}");
        }
        println!("dry-run: seria limpo");
        let on: Vec<&Group> = groups.iter().filter(|g| g.default_on).collect();
        for g in &on {
            println!("  {RED}◉{RED:#} {}", g.line());
        }
        println!("  total {}", human_size(on.iter().map(|g| g.size).sum()));
        if o.verbose {
            print_suggestions(findings);
        }
        return;
    }

    let chosen: Vec<&Group> = if o.yes {
        groups.iter().filter(|g| g.default_on).collect()
    } else {
        let items: Vec<String> = groups.iter().map(menu_item).collect();
        let defaults: Vec<bool> = groups.iter().map(|g| g.default_on).collect();
        MultiSelect::with_theme(&theme())
            .with_prompt("O que limpar? (espaço marca, enter confirma, esc sai)")
            .report(false)
            .items(&items)
            .defaults(&defaults)
            .interact_opt()
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .map(|i| &groups[i])
            .collect()
    };

    let findings_chosen: Vec<&Finding> =
        chosen.iter().flat_map(|g| g.items.iter()).filter(|f| confirm_protected(f, home, o)).collect();
    if findings_chosen.is_empty() {
        println!("Nada selecionado.");
        return;
    }
    let bytes = findings_chosen.iter().filter_map(|f| f.size).sum();
    if !o.yes {
        println!("Selecionado:");
        for g in &chosen {
            println!("  {RED}•{RED:#} {:<28}{RED}{:>10}{RED:#}", g.label, g.size_label());
        }
    }
    if !final_confirm(o, format!("{} grupo(s)", chosen.len()), bytes) {
        println!("Cancelado.");
        return;
    }
    let batches: Vec<(String, String, Vec<&Finding>)> = chosen
        .iter()
        .map(|g| {
            let items = g.items.iter().filter(|f| findings_chosen.iter().any(|c| std::ptr::eq(*c, *f))).collect();
            (g.label.clone(), g.size_label(), items)
        })
        .filter(|(_, _, items): &(String, String, Vec<&Finding>)| !items.is_empty())
        .collect();
    report_outcome(&execute_batches(&batches, o), "item(ns)", home);
    if o.verbose {
        print_suggestions(findings);
    }
}

/// Item-by-item picker (used by `aitrash secrets --clean`).
pub fn run_items(findings: &[Finding], home: &Path, o: &CleanOpts) {
    let cands: Vec<&Finding> = findings.iter().filter(|f| f.actionable() && (o.hard || !f.protected)).collect();
    if cands.is_empty() {
        println!("{GREEN}Nada para limpar.{GREEN:#}");
        return;
    }
    if o.dry_run || (!o.interactive && !o.yes) {
        println!("dry-run: {} arquivo(s) disponíveis; nada é pré-selecionado.", cands.len());
        return;
    }
    let chosen: Vec<&Finding> = if o.yes {
        cands.iter().filter(|f| f.preselected).copied().collect()
    } else {
        pick(&cands, home).into_iter().map(|i| cands[i]).filter(|f| confirm_protected(f, home, o)).collect()
    };
    if chosen.is_empty() {
        println!("Nada selecionado.");
        return;
    }
    let bytes = chosen.iter().filter_map(|f| f.size).sum();
    if !final_confirm(o, format!("{} arquivo(s)", chosen.len()), bytes) {
        println!("Cancelado.");
        return;
    }
    let batch = vec![(format!("{} arquivo(s)", chosen.len()), human_size(bytes), chosen)];
    report_outcome(&execute_batches(&batch, o), "arquivo(s)", home);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_blocks_outside_and_roots() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("home");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("a/x.bak"), "x").unwrap();
        let roots = vec![root.clone()];
        assert!(is_allowed(&root.join("a/x.bak"), &roots));
        assert!(is_allowed(&root.join("a"), &roots));
        assert!(!is_allowed(&root, &roots));
        assert!(!is_allowed(&root.join("a/../../etc"), &roots));
        assert!(!is_allowed(Path::new("/etc/passwd"), &roots));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_removed_not_followed() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep"), "x").unwrap();
        let root = d.path().join("home");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        assert!(is_allowed(&root.join("link"), std::slice::from_ref(&root)));
        remove(&root.join("link"), true).unwrap();
        assert!(outside.join("keep").exists());
    }

    #[cfg(unix)]
    #[test]
    fn parallel_remove_deletes_tree_but_not_symlink_targets() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("keep"), "x").unwrap();
        let root = d.path().join("cache");
        for a in 0..5 {
            for b in 0..5 {
                let dir = root.join(format!("v3/{a}/{b}"));
                fs::create_dir_all(&dir).unwrap();
                for f in 0..4 {
                    fs::write(dir.join(format!("f{f}")), "x").unwrap();
                }
            }
        }
        std::os::unix::fs::symlink(&outside, root.join("v3/0/link")).unwrap();
        remove_dir_parallel(&root).unwrap();
        assert!(!root.exists());
        assert!(outside.join("keep").exists());
    }
}
