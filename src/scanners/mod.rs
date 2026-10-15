pub mod global;
pub mod project;
pub mod secrets;
pub mod system;

use crate::model::{Category, Finding};
use crate::platform::Env;
use std::path::PathBuf;

pub struct Options {
    pub categories: Vec<Category>,
    pub paths: Vec<PathBuf>,
    pub older_than: u64,
    pub max_details: Option<usize>,
    /// Print the current stage on stderr (only when it is a terminal).
    pub progress: bool,
}

fn stage(opt: &Options, msg: &str) {
    if opt.progress {
        eprint!("\r\x1b[2K{msg}");
    }
}

pub fn run(env: &Env, opt: &Options) -> Vec<Finding> {
    let want = |c| opt.categories.contains(&c);
    let mut out = Vec::new();
    if want(Category::Global) {
        stage(opt, "varrendo estado global dos agentes…");
        out.extend(global::scan(env, opt.older_than));
    }
    let mut repos = Vec::new();
    if want(Category::Project) || want(Category::System) {
        stage(opt, "varrendo projetos…");
        let p = project::scan(&opt.paths, &env.home);
        if want(Category::Project) {
            out.extend(p.findings);
        }
        repos = p.repos;
    }
    if want(Category::System) {
        stage(opt, "varrendo caches, processos, git e docker…");
        out.extend(system::scan(env, &repos));
    }
    if want(Category::Secrets) {
        stage(opt, "procurando segredos nas transcrições…");
        out.extend(secrets::scan(env, opt.max_details));
    }
    stage(opt, "");
    out
}
