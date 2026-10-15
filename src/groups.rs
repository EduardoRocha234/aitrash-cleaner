use crate::model::{Category, Finding};
use crate::util::human_size;

/// A bundle of findings cleaned together (one line in the summary and in the clean menu).
pub struct Group {
    pub key: String,
    pub label: String,
    pub items: Vec<Finding>,
    pub size: u64,
    /// Some item has no measured size (Windows-side cache under WSL).
    pub unknown_size: bool,
    pub default_on: bool,
}

impl Group {
    /// Short plain-language explanation shown next to the label.
    pub fn description(&self) -> &'static str {
        match self.key.as_str() {
            "sessions" => "conversas e logs de agentes com 30+ dias",
            "editors" => "cache do Cursor/Windsurf/Claude app; recriado sozinho",
            "project" => "backups, scripts de debug e temporários",
            "project-review" => "resumos .md, logs, bancos de teste; veja com -v",
            "procs" => "dev servers esquecidos ocupando portas",
            "protected" => "credenciais e configs; pede confirmação",
            "cache:uv cache" => "pacotes Python baixados pelo uv/uvx",
            "cache:pip cache" => "pacotes Python baixados pelo pip",
            "cache:npm cache" => "pacotes Node baixados pelo npm",
            "cache:pnpm store" => "pacotes Node baixados pelo pnpm",
            "cache:npx cache (MCP servers)" => "MCP servers e CLIs rodados via npx",
            "cache:playwright browsers" => "navegadores usados para automação/testes",
            "cache:puppeteer browsers" => "Chrome baixado pelo puppeteer",
            _ => "",
        }
    }

    /// Caches are re-downloaded on demand, so removing them is safe but costs bandwidth.
    pub fn line(&self) -> String {
        let extra = if self.key.starts_with("cache:") { "; baixa de novo se precisar" } else { "" };
        format!("{:<28}{:>10}   {}{extra}", self.label, self.size_label(), self.description())
    }

    pub fn size_label(&self) -> String {
        match (self.size, self.unknown_size) {
            (0, true) => "?".into(),
            (s, true) => format!("{}+", human_size(s)),
            (s, false) => human_size(s),
        }
    }
}

fn cache_label(kind: &str) -> String {
    match kind {
        "playwright browsers" => "Browsers playwright".into(),
        "puppeteer browsers" => "Browsers puppeteer".into(),
        "npx cache (MCP servers)" => "Cache npx".into(),
        k => {
            // "uv cache" -> "Cache uv", "pnpm store" -> "Cache pnpm"
            let name = k.trim_end_matches(" cache").trim_end_matches(" store");
            format!("Cache {name}")
        }
    }
}

/// (key, label, default_on) for an actionable, non-secret finding.
fn classify(f: &Finding, hard: bool) -> Option<(String, String, bool)> {
    if !f.actionable() || f.category == Category::Secrets {
        return None;
    }
    if f.protected {
        return hard.then(|| ("protected".into(), "Itens protegidos".into(), false));
    }
    let g = |k: &str, l: &str, on| Some((k.to_string(), l.to_string(), on));
    match f.category {
        Category::Global if f.kind.ends_with(": cache") => g("editors", "Cache de editores IA", true),
        Category::Global if f.preselected => g("sessions", "Sessões antigas dos agentes", true),
        Category::Global => None,
        Category::Project if f.preselected => g("project", "Lixo em projetos", true),
        Category::Project if f.confidence > crate::model::Confidence::Info => {
            g("project-review", "Lixo provável (revisar)", false)
        }
        Category::Project => None,
        Category::System => match f.kind.strip_prefix("cache: ") {
            Some(c) => Some((format!("cache:{c}"), cache_label(c), false)),
            None if matches!(f.action, crate::model::Action::KillPid(_)) => g("procs", "Processos órfãos", false),
            None => None,
        },
        Category::Secrets => None,
    }
}

pub fn group(findings: &[Finding], hard: bool) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for f in findings {
        let Some((key, label, on)) = classify(f, hard) else { continue };
        let idx = match groups.iter().position(|g| g.key == key) {
            Some(i) => i,
            None => {
                groups.push(Group { key, label, items: Vec::new(), size: 0, unknown_size: false, default_on: on });
                groups.len() - 1
            }
        };
        let g = &mut groups[idx];
        match f.size {
            Some(s) => g.size += s,
            None => g.unknown_size = true,
        }
        g.items.push(f.clone());
    }
    groups.sort_by(|a, b| b.default_on.cmp(&a.default_on).then(b.size.cmp(&a.size)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, Confidence};

    fn f(cat: Category, kind: &str, size: Option<u64>) -> Finding {
        let mut f = Finding::new(cat, kind, "/x").action(Action::Trash);
        f.size = size;
        f
    }

    #[test]
    fn merges_caches_across_roots() {
        let v = vec![
            f(Category::System, "cache: uv cache", Some(100)),
            f(Category::System, "cache: uv cache", None),
            f(Category::System, "cache: npm cache", Some(5)),
        ];
        let g = group(&v, false);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].label, "Cache uv");
        assert_eq!(g[0].items.len(), 2);
        assert!(g[0].unknown_size && !g[0].default_on);
        assert!(g[0].size_label().ends_with('+'));
        assert!(g[0].line().contains("pacotes Python"));
        assert!(group(&v, false).iter().all(|g| !g.description().is_empty()));
    }

    #[test]
    fn secrets_suggest_and_protected() {
        let mut prot = f(Category::Global, "codex: credencial", Some(1));
        prot = prot.protect();
        let v = vec![
            f(Category::Secrets, "openai key ×1", Some(1)),
            f(Category::System, "git: stashes", None).action(Action::Suggest("x".into())),
            prot,
        ];
        assert!(group(&v, false).is_empty());
        let g = group(&v, true);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].key, "protected");
    }

    #[test]
    fn project_split_and_order() {
        let v = vec![
            f(Category::Project, "backup", Some(1)).confidence(Confidence::High).preselect(true),
            f(Category::Project, "resumo .md gerado", Some(1)).confidence(Confidence::Low),
            f(Category::Project, "config de agente", Some(1)),
            f(Category::Global, "claude: transcrições antigas", Some(9)).preselect(true),
            f(Category::System, "cache: uv cache", Some(1000)),
        ];
        let keys: Vec<String> = group(&v, false).into_iter().map(|g| g.key).collect();
        assert_eq!(keys, ["sessions", "project", "cache:uv cache", "project-review"]);
    }
}
