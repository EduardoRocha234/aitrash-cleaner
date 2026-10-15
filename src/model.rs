use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Global,
    Project,
    System,
    Secrets,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::Global,
        Category::Project,
        Category::System,
        Category::Secrets,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Global => "ESTADO GLOBAL DOS AGENTES",
            Category::Project => "LIXO EM PROJETOS",
            Category::System => "EFEITOS NO SISTEMA",
            Category::Secrets => "SEGREDOS EM TEXTO PURO",
        }
    }
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Info,
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn bump(self) -> Self {
        match self {
            Confidence::Info => Confidence::Info,
            Confidence::Low => Confidence::Medium,
            Confidence::Medium | Confidence::High => Confidence::High,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Confidence::Info => "info",
            Confidence::Low => "baixa",
            Confidence::Medium => "média",
            Confidence::High => "alta",
        }
    }
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Action {
    /// Move `Finding::targets` (or `path`) to the trash / delete them.
    Trash,
    KillPid(u32),
    /// Only print a command for the user to run.
    Suggest(String),
    None,
}

#[derive(Serialize, Clone, Debug)]
pub struct Finding {
    pub category: Category,
    pub kind: String,
    pub path: PathBuf,
    /// Paths removed by `Action::Trash`. Empty means `[path]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<PathBuf>,
    pub size: Option<u64>,
    pub age_days: Option<u64>,
    pub confidence: Confidence,
    pub preselected: bool,
    /// Needs `--hard` plus per-item confirmation (credentials, git-tracked files).
    pub protected: bool,
    pub note: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
    pub action: Action,
}

impl Finding {
    pub fn new(category: Category, kind: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Finding {
            category,
            kind: kind.into(),
            path: path.into(),
            targets: Vec::new(),
            size: None,
            age_days: None,
            confidence: Confidence::Info,
            preselected: false,
            protected: false,
            note: String::new(),
            details: Vec::new(),
            action: Action::None,
        }
    }

    pub fn confidence(mut self, c: Confidence) -> Self {
        self.confidence = c;
        self
    }
    pub fn size(mut self, s: u64) -> Self {
        self.size = Some(s);
        self
    }
    pub fn age(mut self, a: Option<u64>) -> Self {
        self.age_days = a;
        self
    }
    pub fn note(mut self, n: impl Into<String>) -> Self {
        self.note = n.into();
        self
    }
    pub fn action(mut self, a: Action) -> Self {
        self.action = a;
        self
    }
    pub fn preselect(mut self, p: bool) -> Self {
        self.preselected = p;
        self
    }
    pub fn protect(mut self) -> Self {
        self.protected = true;
        self.preselected = false;
        self
    }
    pub fn targets(mut self, t: Vec<PathBuf>) -> Self {
        self.targets = t;
        self
    }
    pub fn details(mut self, d: Vec<String>) -> Self {
        self.details = d;
        self
    }

    pub fn removal_targets(&self) -> Vec<PathBuf> {
        if self.targets.is_empty() {
            vec![self.path.clone()]
        } else {
            self.targets.clone()
        }
    }

    pub fn actionable(&self) -> bool {
        matches!(self.action, Action::Trash | Action::KillPid(_))
    }
}
