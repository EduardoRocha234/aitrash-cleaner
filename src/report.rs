use crate::model::{Action, Category, Confidence, Finding};
use crate::util::{human_size, pretty_path};
use anstream::println;
use anstyle::{AnsiColor, Style};
use std::path::Path;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const RED: Style = AnsiColor::Red.on_default().bold();
const YELLOW: Style = AnsiColor::Yellow.on_default();
const CYAN: Style = AnsiColor::Cyan.on_default();
const GREEN: Style = AnsiColor::Green.on_default().bold();

fn conf_style(c: Confidence) -> Style {
    match c {
        Confidence::High => RED,
        Confidence::Medium => YELLOW,
        Confidence::Low => CYAN,
        Confidence::Info => DIM,
    }
}

pub fn marker(f: &Finding) -> &'static str {
    if f.protected {
        "[!]"
    } else if f.preselected {
        "[x]"
    } else if f.actionable() {
        "[ ]"
    } else if matches!(f.action, Action::Suggest(_)) {
        " → "
    } else {
        " · "
    }
}

/// One-line label used both in the table and in the interactive picker.
pub fn label(f: &Finding, home: &Path) -> String {
    let size = f.size.map(human_size).unwrap_or_default();
    let age = f.age_days.map(|d| format!("{d}d")).unwrap_or_default();
    let path = if f.path.as_os_str().is_empty() { String::new() } else { pretty_path(&f.path, home) };
    format!("{size:>9} {age:>5}  {}  {path}", f.kind)
}

pub fn print_table(findings: &[Finding], home: &Path, verbose: bool) {
    if findings.is_empty() {
        println!("{GREEN}Nada encontrado. Máquina limpa.{GREEN:#}");
        return;
    }
    for cat in Category::ALL {
        let mut items: Vec<&Finding> = findings.iter().filter(|f| f.category == cat).collect();
        if items.is_empty() {
            continue;
        }
        items.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(b.size.cmp(&a.size)));
        let total: u64 = items.iter().filter_map(|f| f.size).sum();
        println!();
        println!("{BOLD}━━ {}{BOLD:#}  {DIM}({} itens · {}){DIM:#}", cat.title(), items.len(), human_size(total));
        for f in items {
            let cs = conf_style(f.confidence);
            println!(" {} {cs}{:<5}{cs:#} {}", marker(f), f.confidence.label(), label(f, home));
            let mut extra: Vec<String> = Vec::new();
            if !f.note.is_empty() {
                extra.push(f.note.clone());
            }
            if let Action::Suggest(cmd) = &f.action {
                extra.push(format!("$ {cmd}"));
            }
            let limit = if verbose { usize::MAX } else { 3 };
            extra.extend(f.details.iter().take(limit).cloned());
            if !verbose && f.details.len() > 3 {
                extra.push(format!("… +{} (use --verbose)", f.details.len() - 3));
            }
            for e in extra {
                println!("{DIM}{:>27}↳ {e}{DIM:#}", "");
            }
        }
    }
    summary(findings);
}

fn summary(findings: &[Finding]) {
    let pre: Vec<&Finding> = findings.iter().filter(|f| f.preselected).collect();
    let bytes: u64 = pre.iter().filter_map(|f| f.size).sum();
    let secrets = findings.iter().filter(|f| f.category == Category::Secrets).count();
    println!();
    println!("{DIM}legenda: [x] pré-selecionado  [ ] removível  [!] protegido (--hard)  → comando sugerido  · info{DIM:#}");
    if secrets > 0 {
        println!("{RED}⚠ {secrets} arquivo(s) com segredos em texto puro. Rotacione as chaves expostas.{RED:#}");
    }
    println!(
        "{BOLD}Recuperável com segurança: {} em {} item(ns).{BOLD:#} Rode {GREEN}aitrash clean{GREEN:#} para revisar e limpar.",
        human_size(bytes),
        pre.len()
    );
}

/// Default output: one line per cleanable group plus short pointers to the details.
pub fn print_summary(findings: &[Finding]) {
    let groups = crate::groups::group(findings, false);
    let total: u64 = groups.iter().map(|g| g.size).sum();
    println!();
    println!("{RED}aitrash{RED:#} · {BOLD}{}{BOLD:#} encontrados", human_size(total));
    println!();
    if groups.is_empty() {
        println!("  {GREEN}Nada para limpar.{GREEN:#}");
    }
    for g in &groups {
        let mark = if g.default_on { format!("{RED}◉{RED:#}") } else { format!("{DIM}◯{DIM:#}") };
        let n = g.items.len();
        let count = if g.key.starts_with("project") || g.key == "procs" { format!("  {DIM}{n} item(ns){DIM:#}") } else { String::new() };
        println!("  {mark} {:<28}{BOLD}{:>10}{BOLD:#}{count}   {DIM}{}{DIM:#}", g.label, g.size_label(), g.description());
    }

    let secrets = findings.iter().filter(|f| f.category == Category::Secrets).count();
    let keep = findings.iter().filter(|f| f.protected || matches!(f.action, Action::None)).count();
    let suggest = findings.iter().filter(|f| matches!(f.action, Action::Suggest(_))).count();
    println!();
    if secrets > 0 {
        println!("  {RED}⚠ {secrets} arquivo(s) com chaves expostas{RED:#} {DIM}→ aitrash secrets{DIM:#}");
    }
    let mut info = Vec::new();
    if keep > 0 {
        info.push(format!("{keep} credenciais/configs"));
    }
    if suggest > 0 {
        info.push(format!("{suggest} sugestões git/docker"));
    }
    if !info.is_empty() {
        println!("  {DIM}ℹ {} → aitrash -v{DIM:#}", info.join(", "));
    }
    if !groups.is_empty() {
        println!("  {DIM}◉ = selecionado por padrão{DIM:#}   {RED}→ aitrash clean{RED:#}");
    }
}

pub fn print_json(findings: &[Finding]) {
    println!("{}", serde_json::to_string_pretty(findings).unwrap_or_default());
}
