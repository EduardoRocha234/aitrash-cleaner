use crate::model::Confidence;
use regex::bytes::{Regex, RegexSet};
use std::sync::LazyLock;

/// Directories never descended into while looking for project junk.
pub const SKIP_DIRS: [&str; 24] = [
    "node_modules", ".git", "target", ".venv", "venv", "env", "__pycache__", "dist", "build",
    ".next", ".nuxt", ".cache", ".npm", ".cargo", ".rustup", ".local", ".config", "Library",
    "AppData", ".vscode-server", "snap", ".pnpm-store", ".gradle", ".m2",
];

pub struct Rule {
    pub kind: &'static str,
    pub confidence: Confidence,
}

const fn rule(kind: &'static str, confidence: Confidence) -> Option<Rule> {
    Some(Rule { kind, confidence })
}

/// Classifies one directory entry inside a project. `None` = not junk.
pub fn classify(name: &str, is_dir: bool) -> Option<Rule> {
    use Confidence::*;
    let lower = name.to_ascii_lowercase();

    if is_dir {
        return match name {
            ".claude" | ".cursor" | ".windsurf" | ".gemini" | ".codex" | ".kiro" | ".aider.tags.cache.v3" => {
                rule("config de agente", Info)
            }
            "scratch" | "tmp_scripts" | ".scratch" => rule("pasta de rascunho", Medium),
            _ => None,
        };
    }

    // Agent config files: reported, never pre-selected.
    if matches!(
        name,
        "CLAUDE.md" | "CLAUDE.local.md" | "AGENTS.md" | "GEMINI.md" | ".cursorrules" | ".windsurfrules"
            | ".aider.chat.history.md" | ".aider.input.history"
    ) {
        return rule("config de agente", Info);
    }

    // Backups / merge leftovers.
    if lower.ends_with(".bak") || lower.ends_with(".orig") || lower.ends_with(".rej") || name.ends_with('~')
        || lower.contains(".backup") || lower.ends_with(".old")
    {
        return rule("backup", High);
    }

    let (stem, ext) = lower.rsplit_once('.').unwrap_or((lower.as_str(), ""));
    let code = matches!(ext, "py" | "js" | "ts" | "mjs" | "cjs" | "sh" | "rb" | "go" | "rs");

    // Throwaway scripts.
    if code
        && (stem.starts_with("test_fix")
            || stem.starts_with("debug_")
            || stem == "debug"
            || stem.starts_with("check_")
            || stem.starts_with("tmp_")
            || stem.starts_with("temp_")
            || stem.starts_with("scratch")
            || stem.starts_with("quick_test")
            || stem.starts_with("verify_fix")
            || stem.starts_with("repro_"))
    {
        return rule("script descartável", Medium);
    }

    if ext == "md" {
        let upper = name.to_ascii_uppercase();
        let report = ["SUMMARY", "IMPLEMENTATION", "_REPORT", "FIX_", "FIXES", "CHANGES_", "_NOTES", "PROGRESS"]
            .iter()
            .any(|k| upper.contains(k));
        if report && !upper.starts_with("README") && !upper.starts_with("CHANGELOG") {
            return rule("resumo .md gerado", Low);
        }
    }

    if ext == "log" {
        return rule("log", Medium);
    }
    if matches!(ext, "png" | "jpg" | "jpeg" | "webm") && (lower.starts_with("screenshot") || lower.starts_with("screen_")) {
        return rule("screenshot", Medium);
    }
    if matches!(ext, "db" | "sqlite" | "sqlite3") && (stem.contains("test") || stem.contains("tmp") || stem.contains("debug")) {
        return rule("banco de teste", Low);
    }
    if ext == "tmp" {
        return rule("temporário", High);
    }
    None
}

pub struct SecretPattern {
    pub name: &'static str,
    pub re: Regex,
    /// Capture group holding the secret value. For `.env` hits group 1 is the variable name.
    pub group: usize,
}

/// Token boundary that also accepts JSON-escaped whitespace (`\n`, `\t`) since
/// transcripts are JSONL and `\b` sees `\nsk-...` as one word.
const B: &str = r"(?:^|[^A-Za-z0-9_]|\\[nrt])";

/// Cheap prefilter: a line is only examined pattern by pattern if this matches.
pub static SECRET_SET: LazyLock<RegexSet> =
    LazyLock::new(|| RegexSet::new(SECRET_PATTERNS.iter().map(|p| p.re.as_str())).unwrap());

pub static SECRET_PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    // ASCII mode: Unicode `\b` would force the slow regex engine on multi-MB lines.
    let p = |name, re: &str, group| SecretPattern { name, re: Regex::new(&format!("(?-u){re}")).unwrap(), group };
    vec![
        p("anthropic key", r"(sk-ant-[A-Za-z0-9_\-]{20,})", 1),
        p("openai key", &format!(r"{B}(sk-(?:proj-|svcacct-)?[A-Za-z0-9_\-]{{20,}})"), 1),
        p("github token", &format!(r"{B}(gh[pousr]_[A-Za-z0-9]{{36,}}|github_pat_[A-Za-z0-9_]{{22,}})"), 1),
        p("aws access key", &format!(r"{B}((?:AKIA|ASIA)[0-9A-Z]{{16}})\b"), 1),
        p("slack token", &format!(r"{B}(xox[abprs]-[A-Za-z0-9\-]{{10,}})"), 1),
        p("google api key", &format!(r"{B}(AIza[0-9A-Za-z_\-]{{35}})"), 1),
        p("stripe key", &format!(r"{B}([sr]k_live_[0-9A-Za-z]{{20,}})"), 1),
        p("private key", r"(-----BEGIN [A-Z ]*PRIVATE KEY-----)", 1),
        p(
            ".env secret",
            &format!(r#"{B}([A-Z][A-Z0-9_]*(?:SECRET|TOKEN|PASSWORD|PASSWD|API_KEY|APIKEY|PRIVATE_KEY|ACCESS_KEY)[A-Z0-9_]*)\s*=\s*["']?([^\s"'\\$<>{{}}]{{8,}})"#),
            2,
        ),
    ]
});

const PLACEHOLDERS: [&str; 9] = ["xxx", "your", "example", "changeme", "placeholder", "dummy", "redacted", "****", "…"];

pub fn is_placeholder(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    PLACEHOLDERS.iter().any(|p| l.contains(p))
}

/// Variable names that hold paths/patterns/names, not secret values.
const NON_SECRET_SUFFIXES: [&str; 13] = [
    "_PATH", "_FILE", "_RE", "_REGEX", "_PATTERN", "_PREFIX", "_NAME", "_HEADER", "_ENV", "_VAR", "_LENGTH", "_URL", "_COOKIE",
];

/// Values that are code or references rather than literal secrets.
const CODE_PREFIXES: [&str; 14] = [
    "process.", "os.", "env.", "import", "--", "./", "/", "re.", "new", "config.", "settings.", "self.", "this.", "secrets.",
];

/// Heuristic filter against false positives (code, identifiers, placeholders).
/// `var` is the variable name for `.env`-style hits.
pub fn plausible(kind: &str, var: Option<&str>, v: &str) -> bool {
    if is_placeholder(v) {
        return false;
    }
    let has_digit = v.bytes().any(|b| b.is_ascii_digit());
    let has_alpha = v.bytes().any(|b| b.is_ascii_alphabetic());
    match kind {
        // Generic `sk-` prefix: real keys mix letters and digits.
        "openai key" => has_digit && has_alpha && v.len() >= 30,
        ".env secret" => {
            let var = var.unwrap_or_default();
            if NON_SECRET_SUFFIXES.iter().any(|s| var.ends_with(s)) {
                return false;
            }
            let l = v.to_ascii_lowercase();
            if CODE_PREFIXES.iter().any(|p| l.starts_with(p)) || v.contains(['(', ')', ';', '[', ']']) {
                return false;
            }
            // Plain lowercase identifiers ("development", "secret_value") are not secrets.
            let identifier = v.bytes().all(|b| b.is_ascii_lowercase() || b == b'_' || b == b'-');
            !identifier
        }
        _ => true,
    }
}

/// Long tokens keep 6+4 chars (prefix + tail to identify the key); short values only 2.
pub fn mask(v: &str) -> String {
    let chars: Vec<char> = v.chars().collect();
    if v.starts_with("-----BEGIN") {
        return v.to_string();
    }
    if chars.len() < 32 {
        return format!("{}…", chars.iter().take(2).collect::<String>());
    }
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(n: &str) -> Option<(&'static str, Confidence)> {
        classify(n, false).map(|r| (r.kind, r.confidence))
    }

    #[test]
    fn junk_files() {
        assert_eq!(kind("main.rs.bak").unwrap().1, Confidence::High);
        assert_eq!(kind("app.py.orig").unwrap().1, Confidence::High);
        assert_eq!(kind("test_fix_login.py").unwrap().1, Confidence::Medium);
        assert_eq!(kind("debug_api.js").unwrap().1, Confidence::Medium);
        assert_eq!(kind("check_ports.sh").unwrap().1, Confidence::Medium);
        assert_eq!(kind("IMPLEMENTATION_SUMMARY.md").unwrap().1, Confidence::Low);
        assert_eq!(kind("FIX_NOTES.md").unwrap().1, Confidence::Low);
        assert_eq!(kind("CLAUDE.md").unwrap().1, Confidence::Info);
        assert_eq!(kind("server.log").unwrap().1, Confidence::Medium);
        assert_eq!(kind("test.db").unwrap().1, Confidence::Low);
    }

    #[test]
    fn legit_files_ignored() {
        for n in ["README.md", "CHANGELOG.md", "main.py", "test_utils.py", "debugger.rs", "app.db", "logo.png", "check.py"] {
            assert!(kind(n).is_none(), "{n} should not be junk");
        }
    }

    #[test]
    fn dirs() {
        assert_eq!(classify(".claude", true).unwrap().confidence, Confidence::Info);
        assert_eq!(classify("scratch", true).unwrap().confidence, Confidence::Medium);
        assert!(classify("src", true).is_none());
    }

    #[test]
    fn masking_never_leaks() {
        let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ123456";
        let m = mask(secret);
        assert_eq!(m, "sk-ant…3456");
        assert!(!m.contains("ABCDEF"));
        assert_eq!(mask("short1234"), "sh…");
    }

    #[test]
    fn plausibility() {
        assert!(!plausible("openai key", None, "sk-abstraction-layer-for-things"));
        assert!(plausible("openai key", None, "sk-proj-a8F9d0s7f6g5h4j3k2l1Qw2Er3Ty"));
        assert!(!plausible(".env secret", Some("WEBHOOK_SECRET"), "process.env.WEBHOOK_SECRET;"));
        assert!(!plausible(".env secret", Some("SESSION_SECRET"), "development"));
        assert!(!plausible(".env secret", Some("EXPO_API_KEY_PATH"), "./keys/a8f.p8"));
        assert!(plausible(".env secret", Some("DB_PASSWORD"), "hunter2hunter2"));
        assert!(plausible(".env secret", Some("NUXT_SESSION_PASSWORD"), "4c9e34ab12cd7bad"));
    }

    #[test]
    fn openai_needs_word_boundary() {
        let re = &SECRET_PATTERNS.iter().find(|p| p.name == "openai key").unwrap().re;
        assert!(!re.is_match(b"task-abcdefghijklmnopqrstuvwxyz0123"));
        let c = re.captures(br#"{"t":"line\nsk-proj-a8F9d0s7f6g5h4j3k2l1Qw2Er3Ty"}"#).unwrap();
        assert_eq!(&c[1], b"sk-proj-a8F9d0s7f6g5h4j3k2l1Qw2Er3Ty");
        let env = &SECRET_PATTERNS.iter().find(|p| p.name == ".env secret").unwrap().re;
        let c = env.captures(br"x\nOPENAI_API_KEY=abc123def456").unwrap();
        assert_eq!(&c[1], b"OPENAI_API_KEY");
    }

    #[test]
    fn placeholders() {
        assert!(is_placeholder("your_api_key_here"));
        assert!(!is_placeholder("a8f9d0s7f6g5"));
    }
}
