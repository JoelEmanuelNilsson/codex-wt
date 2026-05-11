use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_codex-wt")
}

fn run(args: &[&str], codex_home: &Path) -> Value {
    let output = Command::new(bin())
        .args(args)
        .env("CODEX_HOME", codex_home)
        .output()
        .expect("run codex-wt");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("json output")
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?}\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn fixture_repo() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("tempdir");
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).expect("repo dir");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "codex-wt@example.test"]);
    git(&repo, &["config", "user.name", "Codex WT"]);
    fs::write(repo.join("tracked.txt"), "clean\n").expect("tracked file");
    git(&repo, &["add", "tracked.txt"]);
    git(&repo, &["commit", "-m", "initial"]);
    (temp, repo)
}

#[test]
fn creates_clean_detached_worktree() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");

    let json = run(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "HEAD",
            "--slug",
            "clean-one",
        ],
        codex_home.path(),
    );

    let path = PathBuf::from(json["path"].as_str().unwrap());
    assert!(path.exists());
    assert_eq!(json["detached"], true);
    assert_eq!(json["dirty_applied"], false);
    assert_eq!(
        fs::read_to_string(path.join("tracked.txt")).unwrap(),
        "clean\n"
    );
    assert!(git(&path, &["status", "--short", "--branch"]).starts_with("## HEAD (no branch)"));
}

#[test]
fn applies_dirty_changes_only_when_requested() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    fs::write(repo.join("tracked.txt"), "dirty\n").expect("dirty file");

    let clean = run(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "HEAD",
            "--slug",
            "no-dirty",
        ],
        codex_home.path(),
    );
    let clean_path = PathBuf::from(clean["path"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(clean_path.join("tracked.txt")).unwrap(),
        "clean\n"
    );

    let dirty = run(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "HEAD",
            "--slug",
            "with-dirty",
            "--include-dirty",
        ],
        codex_home.path(),
    );
    let dirty_path = PathBuf::from(dirty["path"].as_str().unwrap());
    assert_eq!(dirty["dirty_applied"], true);
    assert_eq!(
        fs::read_to_string(dirty_path.join("tracked.txt")).unwrap(),
        "dirty\n"
    );
}

#[test]
fn copies_untracked_files_only_when_requested() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    fs::create_dir(repo.join("notes")).expect("notes dir");
    fs::write(repo.join("notes/untracked.txt"), "hello\n").expect("untracked file");

    let clean = run(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "HEAD",
            "--slug",
            "no-untracked",
        ],
        codex_home.path(),
    );
    let clean_path = PathBuf::from(clean["path"].as_str().unwrap());
    assert!(!clean_path.join("notes/untracked.txt").exists());

    let copied = run(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "HEAD",
            "--slug",
            "with-untracked",
            "--include-untracked",
        ],
        codex_home.path(),
    );
    let copied_path = PathBuf::from(copied["path"].as_str().unwrap());
    assert_eq!(copied["untracked_applied"], true);
    assert_eq!(copied["untracked_count"], 1);
    assert_eq!(
        fs::read_to_string(copied_path.join("notes/untracked.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn creates_from_named_branch_and_reports_list_and_inspect_json() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    git(&repo, &["switch", "-c", "feature/test"]);
    fs::write(repo.join("tracked.txt"), "branch\n").expect("branch file");
    git(&repo, &["commit", "-am", "branch change"]);

    let created = run(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "feature/test",
            "--slug",
            "branch-test",
        ],
        codex_home.path(),
    );
    let path = PathBuf::from(created["path"].as_str().unwrap());
    assert_eq!(created["detached"], true);
    assert_eq!(
        fs::read_to_string(path.join("tracked.txt")).unwrap(),
        "branch\n"
    );

    let listed = run(
        &["--json", "list", "--repo", repo.to_str().unwrap()],
        codex_home.path(),
    );
    assert!(listed["worktrees"].as_array().unwrap().len() >= 2);

    let inspected = run(
        &["--json", "inspect", "--path", path.to_str().unwrap()],
        codex_home.path(),
    );
    assert_eq!(inspected["detached"], true);
    assert!(inspected["head"].as_str().unwrap().len() >= 7);
    assert!(
        inspected["status"].as_array().unwrap()[0]
            .as_str()
            .unwrap()
            .starts_with("## HEAD (no branch)")
    );
}

#[test]
fn doctor_reports_setup_as_json() {
    let codex_home = TempDir::new().expect("codex home");
    let json = run(&["--json", "doctor"], codex_home.path());
    assert_eq!(json["ok"], true);
    assert!(
        json["git_version"]
            .as_str()
            .unwrap()
            .starts_with("git version")
    );
}
