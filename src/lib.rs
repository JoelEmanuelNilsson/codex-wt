use std::collections::hash_map::DefaultHasher;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub repo: PathBuf,
    pub base: String,
    pub slug: Option<String>,
    pub include_dirty: bool,
    pub include_untracked: bool,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub git_path: Option<PathBuf>,
    pub git_version: Option<String>,
    pub codex_home: PathBuf,
    pub worktrees_dir: PathBuf,
    pub worktrees_dir_exists: bool,
    pub worktrees_parent_exists: bool,
    pub install_dir: PathBuf,
    pub install_dir_exists: bool,
    pub install_dir_on_path: bool,
}

#[derive(Debug, Serialize)]
pub struct CreateResult {
    pub ok: bool,
    pub path: PathBuf,
    pub repo: PathBuf,
    pub base_ref: String,
    pub head: String,
    pub detached: bool,
    pub dirty_applied: bool,
    pub untracked_applied: bool,
    pub untracked_count: usize,
}

#[derive(Debug, Serialize)]
pub struct WorktreeList {
    pub ok: bool,
    pub repo: PathBuf,
    pub worktrees: Vec<WorktreeEntry>,
}

#[derive(Debug, Serialize)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub prunable: bool,
    pub prunable_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InspectResult {
    pub ok: bool,
    pub path: PathBuf,
    pub repo: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub detached: bool,
    pub git_dir: PathBuf,
    pub source_gitdir: PathBuf,
    pub status: Vec<String>,
}

pub fn doctor() -> DoctorReport {
    let git_path = find_on_path("git");
    let git_version = git_path.as_ref().and_then(|_| {
        Command::new("git")
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    });
    let codex_home = codex_home();
    let worktrees_dir = codex_home.join("worktrees");
    let install_dir = home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("bin");

    DoctorReport {
        ok: git_path.is_some(),
        git_path,
        git_version,
        codex_home,
        worktrees_dir: worktrees_dir.clone(),
        worktrees_dir_exists: worktrees_dir.is_dir(),
        worktrees_parent_exists: worktrees_dir
            .parent()
            .map(|path| path.is_dir())
            .unwrap_or(false),
        install_dir: install_dir.clone(),
        install_dir_exists: install_dir.is_dir(),
        install_dir_on_path: path_contains(&install_dir),
    }
}

pub fn create(options: CreateOptions) -> Result<CreateResult> {
    ensure_git_available()?;
    let repo = repo_root(&options.repo)?;
    if options.include_dirty {
        validate_dirty_base(&repo, &options.base)?;
    }
    if options.include_untracked {
        validate_untracked_sources(&repo)?;
    }
    let repo_name = repo.file_name().and_then(OsStr::to_str).ok_or_else(|| {
        anyhow!(
            "could not infer repository directory name from {}",
            repo.display()
        )
    })?;
    let worktrees_dir = codex_home().join("worktrees");
    fs::create_dir_all(&worktrees_dir)
        .with_context(|| format!("create {}", worktrees_dir.display()))?;

    let id_base = options
        .slug
        .as_deref()
        .map(sanitize_slug)
        .unwrap_or_else(|| generated_id(&repo, &options.base));
    let (worktree_id, path) = unique_worktree_path(&worktrees_dir, &id_base, repo_name);
    let parent = worktrees_dir.join(&worktree_id);
    fs::create_dir_all(&parent).with_context(|| format!("create {}", parent.display()))?;

    let add_result = git(
        Some(&repo),
        [
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--detach"),
            path.as_os_str(),
            OsStr::new(&options.base),
        ],
    );

    if let Err(error) = add_result {
        let _ = fs::remove_dir(&parent);
        return Err(error);
    }

    let post_create = apply_requested_changes(&repo, &path, &options);
    let (dirty_applied, untracked_count) = match post_create {
        Ok(result) => result,
        Err(error) => {
            if let Err(cleanup_error) = cleanup_worktree(&repo, &path, &parent) {
                return Err(error.context(format!(
                    "failed to clean up {} after create error: {cleanup_error:#}",
                    path.display()
                )));
            }
            return Err(error);
        }
    };

    let head = git_trimmed(Some(&path), [OsStr::new("rev-parse"), OsStr::new("HEAD")])?;
    let branch = current_branch(&path)?;

    Ok(CreateResult {
        ok: true,
        path,
        repo,
        base_ref: options.base,
        head,
        detached: branch.is_none(),
        dirty_applied,
        untracked_applied: untracked_count > 0,
        untracked_count,
    })
}

fn apply_requested_changes(
    repo: &Path,
    path: &Path,
    options: &CreateOptions,
) -> Result<(bool, usize)> {
    let mut dirty_applied = false;
    if options.include_dirty {
        let patch = git_bytes(
            Some(repo),
            [
                OsStr::new("diff"),
                OsStr::new("HEAD"),
                OsStr::new("--binary"),
            ],
        )?;
        dirty_applied = !patch.is_empty();
        if dirty_applied {
            git_with_input(
                Some(path),
                [
                    OsStr::new("apply"),
                    OsStr::new("--binary"),
                    OsStr::new("--whitespace=nowarn"),
                ],
                &patch,
            )
            .context("apply tracked dirty changes to worktree")?;
        }
    }

    let untracked_count = if options.include_untracked {
        copy_untracked_files(repo, path)?
    } else {
        0
    };

    Ok((dirty_applied, untracked_count))
}

pub fn list(repo: &Path) -> Result<WorktreeList> {
    ensure_git_available()?;
    let repo = repo_root(repo)?;
    let output = git_trimmed(
        Some(&repo),
        [
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
        ],
    )?;
    Ok(WorktreeList {
        ok: true,
        repo,
        worktrees: parse_worktree_porcelain(&output),
    })
}

pub fn inspect(path: &Path) -> Result<InspectResult> {
    ensure_git_available()?;
    let repo = repo_root(path)?;
    let head = git_trimmed(Some(path), [OsStr::new("rev-parse"), OsStr::new("HEAD")])?;
    let branch = current_branch(path)?;
    let git_dir = git_path(
        Some(path),
        [OsStr::new("rev-parse"), OsStr::new("--git-dir")],
    )?;
    let source_gitdir = git_path(
        Some(path),
        [OsStr::new("rev-parse"), OsStr::new("--git-common-dir")],
    )?;
    let status_output = git_trimmed(
        Some(path),
        [
            OsStr::new("status"),
            OsStr::new("--short"),
            OsStr::new("--branch"),
        ],
    )?;

    Ok(InspectResult {
        ok: true,
        path: path.to_path_buf(),
        repo,
        head,
        branch: branch.clone(),
        detached: branch.is_none(),
        git_dir,
        source_gitdir,
        status: status_output.lines().map(ToOwned::to_owned).collect(),
    })
}

fn repo_root(path: &Path) -> Result<PathBuf> {
    let root = git_trimmed(
        Some(path),
        [OsStr::new("rev-parse"), OsStr::new("--show-toplevel")],
    )
    .with_context(|| format!("resolve Git repository root from {}", path.display()))?;
    Ok(PathBuf::from(root))
}

fn current_branch(path: &Path) -> Result<Option<String>> {
    let branch = git_trimmed(
        Some(path),
        [OsStr::new("branch"), OsStr::new("--show-current")],
    )?;
    Ok((!branch.is_empty()).then_some(branch))
}

fn validate_dirty_base(repo: &Path, base: &str) -> Result<()> {
    let head = resolve_commit(repo, "HEAD")?;
    let base_commit = resolve_commit(repo, base)?;
    if head != base_commit {
        bail!(
            "--include-dirty requires --base to resolve to the source checkout HEAD; HEAD is {head}, but {base} resolves to {base_commit}"
        );
    }
    Ok(())
}

fn resolve_commit(repo: &Path, rev: &str) -> Result<String> {
    let commit_rev = format!("{rev}^{{commit}}");
    git_trimmed(
        Some(repo),
        [
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new(&commit_rev),
        ],
    )
    .with_context(|| format!("resolve commit {rev}"))
}

fn cleanup_worktree(repo: &Path, path: &Path, parent: &Path) -> Result<()> {
    git(
        Some(repo),
        [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            path.as_os_str(),
        ],
    )
    .with_context(|| format!("remove failed worktree {}", path.display()))?;

    match fs::remove_dir(parent) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty
            ) => {}
        Err(error) => return Err(error).with_context(|| format!("remove {}", parent.display())),
    }
    Ok(())
}

fn validate_untracked_sources(repo: &Path) -> Result<()> {
    for rel_path in untracked_relative_paths(repo)? {
        let source = repo.join(&rel_path);
        let metadata = fs::symlink_metadata(&source)
            .with_context(|| format!("inspect untracked {}", rel_path.display()))?;
        if metadata.is_dir() {
            bail!(
                "refusing unsupported untracked directory entry: {}; nested Git repositories are not copied",
                rel_path.display()
            );
        }
        #[cfg(not(unix))]
        if metadata.file_type().is_symlink() {
            bail!(
                "refusing unsupported untracked symlink source on this platform: {}",
                rel_path.display()
            );
        }
    }
    Ok(())
}

fn untracked_relative_paths(repo: &Path) -> Result<Vec<PathBuf>> {
    let output = git_bytes(
        Some(repo),
        [
            OsStr::new("ls-files"),
            OsStr::new("--others"),
            OsStr::new("--exclude-standard"),
            OsStr::new("-z"),
        ],
    )?;
    output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|raw_path| {
            let rel =
                String::from_utf8(raw_path.to_vec()).context("untracked path is not UTF-8")?;
            safe_relative_path(&rel)
        })
        .collect()
}

fn copy_untracked_files(repo: &Path, target: &Path) -> Result<usize> {
    let mut count = 0;
    for rel_path in untracked_relative_paths(repo)? {
        let source = repo.join(&rel_path);
        let destination = target.join(&rel_path);
        prepare_destination(target, &destination)?;
        copy_file_or_symlink(&source, &destination)
            .with_context(|| format!("copy untracked {}", rel_path.display()))?;
        count += 1;
    }
    Ok(count)
}

fn prepare_destination(target: &Path, destination: &Path) -> Result<()> {
    if !destination.starts_with(target) {
        bail!(
            "refusing destination outside worktree: {}",
            destination.display()
        );
    }
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow!("destination has no parent: {}", destination.display()))?;
    let relative_parent = parent
        .strip_prefix(target)
        .with_context(|| format!("resolve {} under {}", parent.display(), target.display()))?;

    let mut current = target.to_path_buf();
    for component in relative_parent.components() {
        let Component::Normal(part) = component else {
            bail!("refusing unsafe destination parent: {}", parent.display());
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "refusing symlink destination ancestor: {}",
                    current.display()
                );
            }
            Ok(metadata) if !metadata.is_dir() => {
                bail!(
                    "destination ancestor is not a directory: {}",
                    current.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {
                fs::create_dir(&current).with_context(|| format!("create {}", current.display()))?
            }
            Err(error) => {
                return Err(error).with_context(|| format!("inspect {}", current.display()));
            }
        }
    }

    match fs::symlink_metadata(destination) {
        Ok(_) => bail!(
            "refusing to overwrite existing path: {}",
            destination.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", destination.display())),
    }
}

fn copy_file_or_symlink(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        #[cfg(not(unix))]
        {
            bail!(
                "refusing to copy symlink source on this platform: {}",
                source.display()
            );
        }

        #[cfg(unix)]
        {
            let target = fs::read_link(source)?;
            std::os::unix::fs::symlink(target, destination)?;
        }
    } else {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        io::copy(&mut input, &mut output)?;
        fs::set_permissions(destination, metadata.permissions())?;
    }
    Ok(())
}

fn safe_relative_path(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    if path.is_absolute() {
        bail!("refusing absolute untracked path: {raw}");
    }
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            _ => bail!("refusing unsafe untracked path: {raw}"),
        }
    }
    Ok(clean)
}

fn unique_worktree_path(root: &Path, id_base: &str, repo_name: &str) -> (String, PathBuf) {
    for index in 0..1000 {
        let id = if index == 0 {
            id_base.to_string()
        } else {
            format!("{id_base}-{index}")
        };
        let path = root.join(&id).join(repo_name);
        if !path.exists() {
            return (id, path);
        }
    }
    panic!("could not find unique worktree path for {id_base}");
}

pub fn sanitize_slug(raw: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        let allowed = ch.is_ascii_alphanumeric();
        if allowed {
            output.push(ch);
            previous_dash = false;
        } else if !previous_dash && !output.is_empty() {
            output.push('-');
            previous_dash = true;
        }
        if output.len() >= 48 {
            break;
        }
    }
    while output.ends_with('-') {
        output.pop();
    }
    if output.is_empty() {
        "worktree".to_string()
    } else {
        output
    }
}

fn generated_id(repo: &Path, base: &str) -> String {
    let mut hasher = DefaultHasher::new();
    repo.hash(&mut hasher);
    base.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .hash(&mut hasher);
    format!("{:08x}", hasher.finish() & 0xffff_ffff)
}

fn parse_worktree_porcelain(output: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for line in output.lines() {
        if line.is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }

        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(path),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                prunable: false,
                prunable_reason: None,
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };
        if let Some(head) = line.strip_prefix("HEAD ") {
            entry.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            entry.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
        } else if line == "detached" {
            entry.detached = true;
        } else if line == "bare" {
            entry.bare = true;
        } else if let Some(reason) = line.strip_prefix("prunable ") {
            entry.prunable = true;
            entry.prunable_reason = Some(reason.to_string());
        } else if line == "prunable" {
            entry.prunable = true;
        }
    }

    if let Some(entry) = current {
        entries.push(entry);
    }
    entries
}

fn ensure_git_available() -> Result<()> {
    find_on_path("git")
        .map(|_| ())
        .ok_or_else(|| anyhow!("git was not found on PATH"))
}

fn git_trimmed<const N: usize>(cwd: Option<&Path>, args: [&OsStr; N]) -> Result<String> {
    let bytes = git_bytes(cwd, args)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn git_path<const N: usize>(cwd: Option<&Path>, args: [&OsStr; N]) -> Result<PathBuf> {
    let raw = git_trimmed(cwd, args)?;
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        Ok(path)
    } else if let Some(cwd) = cwd {
        Ok(cwd.join(path))
    } else {
        Ok(path)
    }
}

fn git<const N: usize>(cwd: Option<&Path>, args: [&OsStr; N]) -> Result<()> {
    let output = command("git", cwd, args).output().context("run git")?;
    if !output.status.success() {
        bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn git_bytes<const N: usize>(cwd: Option<&Path>, args: [&OsStr; N]) -> Result<Vec<u8>> {
    let output = command("git", cwd, args).output().context("run git")?;
    if !output.status.success() {
        bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git_with_input<const N: usize>(
    cwd: Option<&Path>,
    args: [&OsStr; N],
    input: &[u8],
) -> Result<()> {
    let mut child = command("git", cwd, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("run git")?;
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(input)
        .context("write git stdin")?;
    let output = child.wait_with_output().context("wait for git")?;
    if !output.status.success() {
        bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn command<const N: usize>(program: &str, cwd: Option<&Path>, args: [&OsStr; N]) -> Command {
    let mut command = Command::new(program);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.args(args);
    command
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn path_contains(dir: &Path) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|entry| entry == dir))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_slugs_for_paths() {
        assert_eq!(sanitize_slug("EP-623 Public API!!"), "ep-623-public-api");
        assert_eq!(sanitize_slug("___"), "worktree");
        assert_eq!(sanitize_slug("Already_ok"), "already-ok");
    }

    #[test]
    fn rejects_unsafe_relative_paths() {
        assert!(safe_relative_path("../secret").is_err());
        assert!(safe_relative_path("/tmp/secret").is_err());
        assert_eq!(
            safe_relative_path("docs/readme.md").unwrap(),
            PathBuf::from("docs/readme.md")
        );
    }

    #[test]
    fn parses_porcelain_worktree_output() {
        let entries = parse_worktree_porcelain(
            "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /wt\nHEAD def\ndetached\nprunable stale\n",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(entries[1].detached);
        assert!(entries[1].prunable);
    }
}
