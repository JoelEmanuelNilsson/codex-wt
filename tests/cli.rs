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

fn run_failure(args: &[&str], codex_home: &Path) -> Value {
    let output = Command::new(bin())
        .args(args)
        .env("CODEX_HOME", codex_home)
        .output()
        .expect("run codex-wt");
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("json error output")
}

fn run_raw(args: &[&str], codex_home: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("CODEX_HOME", codex_home)
        .output()
        .expect("run codex-wt")
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
fn rejects_dirty_changes_when_base_is_not_source_head() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    git(&repo, &["switch", "-c", "feature"]);
    fs::write(repo.join("tracked.txt"), "feature committed\n").expect("feature file");
    git(&repo, &["commit", "-am", "feature change"]);
    fs::write(repo.join("tracked.txt"), "feature dirty\n").expect("dirty file");

    let json = run_failure(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "main",
            "--slug",
            "dirty-main",
            "--include-dirty",
        ],
        codex_home.path(),
    );

    assert_eq!(json["ok"], false);
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--include-dirty requires --base")
    );
    assert!(!codex_home.path().join("worktrees/dirty-main/repo").exists());
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
fn failed_untracked_copy_removes_registered_worktree() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    git(&repo, &["switch", "-c", "base-conflict"]);
    fs::write(repo.join("conflict.txt"), "base tracked\n").expect("base conflict");
    git(&repo, &["add", "conflict.txt"]);
    git(&repo, &["commit", "-m", "base conflict"]);
    git(&repo, &["switch", "main"]);
    fs::write(repo.join("conflict.txt"), "source untracked\n").expect("source conflict");

    let json = run_failure(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "base-conflict",
            "--slug",
            "conflict-cleanup",
            "--include-untracked",
        ],
        codex_home.path(),
    );

    let failed_path = codex_home.path().join("worktrees/conflict-cleanup/repo");
    assert_eq!(json["ok"], false);
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("refusing to overwrite existing path")
    );
    assert!(!failed_path.exists());
    let listed = git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(!listed.contains(failed_path.to_str().unwrap()));
}

#[cfg(unix)]
#[test]
fn rejects_untracked_copy_through_symlink_ancestor() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    let outside = TempDir::new().expect("outside");

    git(&repo, &["switch", "-c", "base-symlink"]);
    std::os::unix::fs::symlink(outside.path(), repo.join("linkdir")).expect("symlink");
    git(&repo, &["add", "linkdir"]);
    git(&repo, &["commit", "-m", "base symlink"]);
    git(&repo, &["switch", "main"]);
    fs::create_dir(repo.join("linkdir")).expect("source linkdir dir");
    fs::write(repo.join("linkdir/file.txt"), "do not escape\n").expect("untracked nested file");

    let json = run_failure(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "base-symlink",
            "--slug",
            "symlink-cleanup",
            "--include-untracked",
        ],
        codex_home.path(),
    );

    assert_eq!(json["ok"], false);
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("refusing symlink destination ancestor")
    );
    assert!(!outside.path().join("file.txt").exists());
    assert!(
        !codex_home
            .path()
            .join("worktrees/symlink-cleanup/repo")
            .exists()
    );
}

#[test]
fn rejects_untracked_nested_git_repo_before_creating_worktree() {
    let (_temp, repo) = fixture_repo();
    let codex_home = TempDir::new().expect("codex home");
    let nested = repo.join("nested");
    fs::create_dir(&nested).expect("nested dir");
    git(&nested, &["init", "-b", "main"]);

    let json = run_failure(
        &[
            "--json",
            "create",
            "--repo",
            repo.to_str().unwrap(),
            "--base",
            "HEAD",
            "--slug",
            "nested-repo",
            "--include-untracked",
        ],
        codex_home.path(),
    );

    assert_eq!(json["ok"], false);
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("refusing unsupported untracked directory entry")
    );
    assert!(
        !codex_home
            .path()
            .join("worktrees/nested-repo/repo")
            .exists()
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

#[test]
fn json_parse_errors_use_json_error_shape() {
    let codex_home = TempDir::new().expect("codex home");
    let json = run_failure(&["--json", "create", "--repo", "."], codex_home.path());
    assert_eq!(json["ok"], false);
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("required")
    );
}

#[test]
fn json_help_keeps_help_success_semantics() {
    let codex_home = TempDir::new().expect("codex home");
    let output = run_raw(&["--json", "--help"], codex_home.path());
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: codex-wt"));
    assert!(!stdout.contains("\"ok\": false"));
}
