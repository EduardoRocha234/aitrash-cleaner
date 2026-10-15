mod clean;
mod groups;
mod model;
mod platform;
mod report;
mod rules;
mod scanners;
mod util;

use anstream::println;
use anstyle::Style;
use clap::{Args, Parser, Subcommand};
use model::Category;
use platform::{Env, APPS, DOT_DIRS};
use std::io::IsTerminal;
use std::path::PathBuf;
use util::{human_size, pretty_path};

#[derive(Parser)]
#[command(
    name = "aitrash",
    version,
    about = "Caça lixo deixado por agentes de IA (Claude Code, Codex, Gemini/Antigravity, Cursor, Windsurf)",
    after_help = "Exemplos:\n  aitrash                       varre o diretório atual + estado global\n  aitrash scan --home           varre todos os projetos do home\n  aitrash clean                 escolhe o que limpar (vai para a lixeira)\n  aitrash -v                    lista cada item em detalhe\n  aitrash secrets               procura chaves vazadas nas transcrições\n  aitrash du                    quanto cada agente ocupa"
)]
struct Cli {
    /// No WSL, ignora o perfil do Windows em /mnt/c (bem mais rápido)
    #[arg(long, global = true)]
    no_wsl: bool,
    /// Mostra cada item em detalhe
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Varre e mostra o que encontrou (não altera nada)
    Scan(ScanArgs),
    /// Varre e limpa interativamente (lixeira por padrão)
    Clean {
        #[command(flatten)]
        scan: ScanArgs,
        /// Aplica os itens pré-selecionados sem perguntar
        #[arg(short, long)]
        yes: bool,
        /// Apaga permanentemente e inclui itens protegidos (com confirmação por item)
        #[arg(long)]
        hard: bool,
        /// Só mostra o que seria removido
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Procura segredos (chaves, tokens, .env) nas transcrições dos agentes
    Secrets {
        /// Mostra todas as ocorrências, não só as 3 primeiras por arquivo
        #[arg(short, long)]
        all: bool,
        #[arg(long)]
        json: bool,
        /// Escolhe transcrições com segredos para mandar à lixeira
        #[arg(long)]
        clean: bool,
        /// Com --clean: só mostra o que seria removido
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// Tamanho de cada pasta de agente e cache (auditoria rápida)
    Du {
        #[arg(long)]
        json: bool,
    },
    /// Lista os caminhos conhecidos neste sistema
    Paths,
}

#[derive(Args, Default)]
struct ScanArgs {
    /// Projetos a varrer (padrão: diretório atual)
    paths: Vec<PathBuf>,
    /// Varre o home inteiro procurando projetos
    #[arg(long)]
    home: bool,
    /// Limita às categorias (separadas por vírgula)
    #[arg(long, value_delimiter = ',')]
    only: Vec<Category>,
    /// Idade mínima de transcrições/sessões para virar lixo (ex.: 30d, 2w, 6m)
    #[arg(long, default_value = "30d", value_parser = util::parse_days)]
    older_than: u64,
    /// Saída em JSON
    #[arg(long)]
    json: bool,
}

impl ScanArgs {
    fn options(&self, env: &Env) -> scanners::Options {
        let paths = if self.home {
            vec![env.home.clone()]
        } else if self.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            self.paths.clone()
        };
        scanners::Options {
            categories: if self.only.is_empty() { Category::ALL.to_vec() } else { self.only.clone() },
            paths: paths.into_iter().map(|p| std::fs::canonicalize(&p).unwrap_or(p)).collect(),
            older_than: self.older_than,
            max_details: None,
            progress: !self.json && std::io::stderr().is_terminal(),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let env = Env::detect(!cli.no_wsl);
    let cmd = cli.cmd.unwrap_or(Cmd::Scan(ScanArgs { older_than: 30, ..Default::default() }));

    let verbose = cli.verbose;
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let roots = |extra: &[PathBuf]| -> Vec<PathBuf> {
        env.roots.iter().map(|r| r.home.clone()).chain(extra.iter().cloned()).collect()
    };

    match cmd {
        Cmd::Scan(a) => {
            let f = scanners::run(&env, &a.options(&env));
            if a.json {
                report::print_json(&f)
            } else if verbose {
                report::print_table(&f, &env.home, true)
            } else {
                report::print_summary(&f)
            }
        }
        Cmd::Clean { scan, yes, hard, dry_run } => {
            let mut opt = scan.options(&env);
            // Secrets have their own flow (`aitrash secrets --clean`) unless asked for explicitly.
            if scan.only.is_empty() {
                opt.categories.retain(|c| *c != Category::Secrets);
            }
            let f = scanners::run(&env, &opt);
            let groups = groups::group(&f, hard);
            let o = clean::CleanOpts { yes, hard, dry_run, interactive, verbose, allowed_roots: roots(&opt.paths) };
            clean::run_groups(&groups, &f, &env.home, &o);
        }
        Cmd::Secrets { all, json, clean, dry_run } => {
            let f = scanners::secrets::scan(&env, if all || verbose { None } else { Some(3) });
            if json {
                report::print_json(&f)
            } else if clean {
                let o = clean::CleanOpts { yes: false, hard: false, dry_run, interactive, verbose, allowed_roots: roots(&[]) };
                clean::run_items(&f, &env.home, &o);
            } else {
                report::print_table(&f, &env.home, all || verbose)
            }
        }
        Cmd::Du { json } => du(&env, json),
        Cmd::Paths => paths(&env),
    }
}

/// Every location the tool knows about, per root: (label, path).
fn known_paths(env: &Env) -> Vec<(String, PathBuf)> {
    let mut v = Vec::new();
    for r in &env.roots {
        for d in DOT_DIRS {
            v.push((d.trim_start_matches('.').to_string(), r.home.join(d)));
        }
        v.push(("claude.json".into(), r.home.join(".claude.json")));
        for app in APPS {
            for p in r.app_dirs(app) {
                v.push((format!("{} (app)", app.to_lowercase()), p));
            }
        }
        for (label, p, _) in r.caches() {
            v.push((format!("cache: {label}"), p));
        }
    }
    v
}

fn du(env: &Env, json: bool) {
    let found: Vec<(String, PathBuf)> = known_paths(env).into_iter().filter(|(_, p)| p.exists()).collect();
    let paths: Vec<PathBuf> = found.iter().map(|(_, p)| p.clone()).collect();
    let mut rows: Vec<(String, PathBuf, u64)> =
        found.into_iter().zip(util::sizes(&paths)).map(|((l, p), s)| (l, p, s)).collect();
    rows.sort_by_key(|a| std::cmp::Reverse(a.2));
    if json {
        let v: Vec<_> = rows.iter().map(|(l, p, s)| serde_json::json!({"label": l, "path": p, "size": s})).collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    let bold = Style::new().bold();
    let total: u64 = rows.iter().map(|r| r.2).sum();
    for (l, p, s) in &rows {
        println!("{:>10}  {:<28} {}", human_size(*s), l, pretty_path(p, &env.home));
    }
    println!("{bold}{:>10}  total{bold:#}", human_size(total));
}

fn paths(env: &Env) {
    let dim = Style::new().dimmed();
    for (l, p) in known_paths(env) {
        if p.exists() {
            println!("  ✓ {:<28} {}", l, pretty_path(&p, &env.home));
        } else {
            println!("{dim}  ✗ {:<28} {}{dim:#}", l, pretty_path(&p, &env.home));
        }
    }
}
