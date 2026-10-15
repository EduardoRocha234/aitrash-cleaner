use assert_cmd::Command;
use std::fs;
use std::path::Path;

fn aitrash(home: &Path) -> Command {
    let mut c = Command::cargo_bin("aitrash").unwrap();
    c.env("AITRASH_HOME", home).env("NO_COLOR", "1");
    c
}

fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let proj = d.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("app.py.bak"), "old").unwrap();
    fs::write(proj.join("main.py"), "print()").unwrap();
    fs::write(proj.join("IMPLEMENTATION_SUMMARY.md"), "# done").unwrap();
    let t = d.path().join(".claude/projects/-proj");
    fs::create_dir_all(&t).unwrap();
    fs::write(t.join("s.jsonl"), "{\"t\":\"GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789AB\"}\n").unwrap();
    d
}

#[test]
fn scan_json_lists_findings() {
    let d = fixture();
    let out = aitrash(d.path()).args(["scan", "--json"]).arg(d.path().join("proj")).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let kinds: Vec<&str> = v.as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"backup"));
    assert!(kinds.contains(&"resumo .md gerado"));
    assert!(kinds.iter().any(|k| k.contains("github token")));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("ghp_abcdefghijklmnop"));
}

#[test]
fn clean_without_tty_is_dry_run() {
    let d = fixture();
    let out = aitrash(d.path()).args(["clean", "--only", "project"]).arg(d.path().join("proj")).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("dry-run"));
    assert!(d.path().join("proj/app.py.bak").exists());
}

#[test]
fn clean_yes_hard_removes_only_preselected() {
    let d = fixture();
    aitrash(d.path())
        .args(["clean", "--only", "project", "--yes", "--hard"])
        .arg(d.path().join("proj"))
        .assert()
        .success();
    assert!(!d.path().join("proj/app.py.bak").exists());
    assert!(d.path().join("proj/IMPLEMENTATION_SUMMARY.md").exists(), "low confidence is not preselected");
    assert!(d.path().join("proj/main.py").exists());
}

#[test]
fn secrets_command_masks() {
    let d = fixture();
    let out = aitrash(d.path()).args(["secrets"]).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("github token"));
    assert!(s.contains("ghp_ab…89AB"));
    assert!(!s.contains("ghp_abcdefghij"));
}

#[test]
fn du_and_paths_run() {
    let d = fixture();
    aitrash(d.path()).arg("du").assert().success();
    aitrash(d.path()).arg("paths").assert().success();
}

#[test]
fn default_scan_is_short_summary() {
    let d = fixture();
    let out = aitrash(d.path()).args(["scan", "--only", "global,project,secrets"]).arg(d.path().join("proj")).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.lines().count() <= 20, "{s}");
    assert!(s.contains("Lixo em projetos"));
    assert!(s.contains("aitrash secrets"));
    assert!(s.contains("aitrash clean"));
    assert!(!s.contains("app.py.bak"), "summary must not list items");
    let v = aitrash(d.path()).args(["-v", "scan", "--only", "project"]).arg(d.path().join("proj")).output().unwrap();
    assert!(String::from_utf8_lossy(&v.stdout).contains("app.py.bak"));
}

#[test]
fn clean_dry_run_lists_groups_and_skips_secrets() {
    let d = fixture();
    let out = aitrash(d.path()).args(["clean", "-n"]).arg(d.path().join("proj")).output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("◉ Lixo em projetos"), "{s}");
    assert!(!s.contains("github"));
}
