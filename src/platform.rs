use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    Linux,
    Windows,
    Mac,
}

/// A user home to inspect, with the OS layout its app data follows.
/// Under WSL the Windows profile (`/mnt/c/Users/<name>`) is an extra root with `Os::Windows`.
#[derive(Clone, Debug)]
pub struct Root {
    pub home: PathBuf,
    pub os: Os,
    /// Filesystem where stat is very slow (WSL `/mnt/c` over 9p): skip deep size walks.
    pub slow: bool,
}

#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub roots: Vec<Root>,
}

/// VS Code forks and desktop apps that keep state in the per-OS app-data dir.
pub const APPS: [&str; 5] = ["Cursor", "Windsurf", "Antigravity", "Claude", "Kiro"];

/// Hidden dirs/files every harness drops in the home, same on every OS.
pub const DOT_DIRS: [&str; 7] = [".claude", ".codex", ".gemini", ".cursor", ".windsurf", ".codeium", ".kiro"];

impl Env {
    /// Real environment. `AITRASH_HOME` overrides the home (used by tests) and disables WSL roots.
    pub fn detect(include_wsl: bool) -> Env {
        if let Some(h) = std::env::var_os("AITRASH_HOME") {
            return Env::single(PathBuf::from(h), current_os());
        }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let mut env = Env::single(home, current_os());
        if include_wsl && is_wsl() {
            env.roots.extend(wsl_windows_homes(Path::new("/mnt/c/Users")));
        }
        env
    }

    pub fn single(home: PathBuf, os: Os) -> Env {
        Env {
            roots: vec![Root { home: home.clone(), os, slow: false }],
            home,
        }
    }
}

impl Root {
    /// Per-OS app-data directories for `app` (config/state, caches inside).
    pub fn app_dirs(&self, app: &str) -> Vec<PathBuf> {
        let h = &self.home;
        match self.os {
            Os::Linux => vec![h.join(".config").join(app)],
            Os::Mac => vec![h.join("Library/Application Support").join(app)],
            Os::Windows => vec![
                h.join("AppData/Roaming").join(app),
                h.join("AppData/Local").join(app),
            ],
        }
    }

    /// Heavy package/browser caches commonly filled by agents and MCP servers.
    /// (label, path, official clean command)
    pub fn caches(&self) -> Vec<(&'static str, PathBuf, &'static str)> {
        let h = &self.home;
        match self.os {
            Os::Linux => vec![
                ("playwright browsers", h.join(".cache/ms-playwright"), "npx playwright uninstall --all"),
                ("npx cache (MCP servers)", h.join(".npm/_npx"), "rm -rf ~/.npm/_npx"),
                ("npm cache", h.join(".npm/_cacache"), "npm cache clean --force"),
                ("pip cache", h.join(".cache/pip"), "pip cache purge"),
                ("uv cache", h.join(".cache/uv"), "uv cache clean"),
                ("pnpm store", h.join(".local/share/pnpm/store"), "pnpm store prune"),
                ("puppeteer browsers", h.join(".cache/puppeteer"), "rm -rf ~/.cache/puppeteer"),
            ],
            Os::Mac => vec![
                ("playwright browsers", h.join("Library/Caches/ms-playwright"), "npx playwright uninstall --all"),
                ("npx cache (MCP servers)", h.join(".npm/_npx"), "rm -rf ~/.npm/_npx"),
                ("npm cache", h.join(".npm/_cacache"), "npm cache clean --force"),
                ("pip cache", h.join("Library/Caches/pip"), "pip cache purge"),
                ("uv cache", h.join(".cache/uv"), "uv cache clean"),
                ("pnpm store", h.join("Library/pnpm/store"), "pnpm store prune"),
                ("puppeteer browsers", h.join(".cache/puppeteer"), "rm -rf ~/.cache/puppeteer"),
            ],
            Os::Windows => vec![
                ("playwright browsers", h.join("AppData/Local/ms-playwright"), "npx playwright uninstall --all"),
                ("npx cache (MCP servers)", h.join("AppData/Local/npm-cache/_npx"), "rmdir /s %LOCALAPPDATA%\\npm-cache\\_npx"),
                ("npm cache", h.join("AppData/Local/npm-cache/_cacache"), "npm cache clean --force"),
                ("pip cache", h.join("AppData/Local/pip/Cache"), "pip cache purge"),
                ("uv cache", h.join("AppData/Local/uv/cache"), "uv cache clean"),
                ("pnpm store", h.join("AppData/Local/pnpm/store"), "pnpm store prune"),
                ("puppeteer browsers", h.join(".cache/puppeteer"), "rmdir /s %USERPROFILE%\\.cache\\puppeteer"),
            ],
        }
    }

    pub fn shell_rc_files(&self) -> Vec<PathBuf> {
        if self.os == Os::Windows {
            return vec![self.home.join("Documents/PowerShell/Microsoft.PowerShell_profile.ps1")];
        }
        [".bashrc", ".zshrc", ".profile", ".bash_profile", ".zprofile", ".config/fish/config.fish"]
            .iter()
            .map(|f| self.home.join(f))
            .collect()
    }
}

pub fn current_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    }
}

fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

fn wsl_windows_homes(users: &Path) -> Vec<Root> {
    const SKIP: [&str; 4] = ["Public", "Default", "Default User", "All Users"];
    let Ok(rd) = std::fs::read_dir(users) else { return Vec::new() };
    rd.filter_map(Result::ok)
        .filter(|e| !SKIP.contains(&e.file_name().to_string_lossy().as_ref()))
        .map(|e| e.path())
        .filter(|p| p.join("AppData").is_dir())
        .map(|home| Root { home, os: Os::Windows, slow: true })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_homes_skip_system_profiles() {
        let d = tempfile::tempdir().unwrap();
        for u in ["alice", "Public", "Default"] {
            std::fs::create_dir_all(d.path().join(u).join("AppData")).unwrap();
        }
        std::fs::create_dir_all(d.path().join("noappdata")).unwrap();
        let roots = wsl_windows_homes(d.path());
        assert_eq!(roots.len(), 1);
        assert!(roots[0].home.ends_with("alice"));
        assert_eq!(roots[0].os, Os::Windows);
    }

    #[test]
    fn app_dirs_per_os() {
        let r = Root { home: "/h".into(), os: Os::Mac, slow: false };
        assert_eq!(r.app_dirs("Cursor")[0], PathBuf::from("/h/Library/Application Support/Cursor"));
        let r = Root { home: "/h".into(), os: Os::Windows, slow: true };
        assert_eq!(r.app_dirs("Cursor").len(), 2);
    }
}
