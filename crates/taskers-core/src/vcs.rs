use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use taskers_control::{
    VcsCommand, VcsCommandResult, VcsFileEntry, VcsFileStatus, VcsMode, VcsPullRequestInfo,
    VcsRefEntry, VcsSnapshot,
};
use taskers_domain::{AppModel, PaneKind, SurfaceId};

#[derive(Clone, Default)]
pub struct VcsService;

#[derive(Debug, Clone)]
struct RepoTarget {
    surface_id: SurfaceId,
    cwd: PathBuf,
    repo_root: PathBuf,
    mode: VcsMode,
}

impl VcsService {
    pub fn execute(&self, model: &AppModel, command: VcsCommand) -> Result<VcsCommandResult> {
        match command {
            VcsCommand::Refresh {
                surface_id,
                diff_path,
            } => Ok(VcsCommandResult {
                snapshot: Some(self.snapshot_for_surface(model, surface_id, diff_path)?),
                message: None,
            }),
            VcsCommand::GitCommit {
                surface_id,
                message,
            } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Git, "git commit")?;
                ensure_non_empty(&message, "commit message")?;
                let _output =
                    run_command(&target.repo_root, "git", &["commit", "-m", message.trim()])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::GitCreateBranch { surface_id, name } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Git, "git branch create")?;
                ensure_non_empty(&name, "branch name")?;
                let _output =
                    run_command(&target.repo_root, "git", &["switch", "-c", name.trim()])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::GitSwitchBranch { surface_id, name } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Git, "git branch switch")?;
                ensure_non_empty(&name, "branch name")?;
                let _output = run_command(&target.repo_root, "git", &["switch", name.trim()])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::GitFetch { surface_id } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Git, "git fetch")?;
                let _output =
                    run_command(&target.repo_root, "git", &["fetch", "--all", "--prune"])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::GitPull { surface_id } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Git, "git pull")?;
                let _output = run_command(&target.repo_root, "git", &["pull", "--ff-only"])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::GitPush { surface_id } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Git, "git push")?;
                let _output = run_command(&target.repo_root, "git", &["push"])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::JjDescribe {
                surface_id,
                message,
            } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Jj, "jj describe")?;
                ensure_non_empty(&message, "change description")?;
                let _output =
                    run_command(&target.repo_root, "jj", &["describe", "-m", message.trim()])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::JjNew {
                surface_id,
                message,
            } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Jj, "jj new")?;
                let _output = if let Some(message) =
                    message.as_deref().map(str::trim).filter(|s| !s.is_empty())
                {
                    run_command(&target.repo_root, "jj", &["new", "-m", message])?
                } else {
                    run_command(&target.repo_root, "jj", &["new"])?
                };
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::JjCreateBookmark { surface_id, name } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Jj, "jj bookmark create")?;
                ensure_non_empty(&name, "bookmark name")?;
                let _output = run_command(
                    &target.repo_root,
                    "jj",
                    &["bookmark", "create", name.trim()],
                )?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::JjSwitchBookmark { surface_id, name } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Jj, "jj edit")?;
                ensure_non_empty(&name, "bookmark name")?;
                let _output = run_command(&target.repo_root, "jj", &["edit", name.trim()])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::JjFetch { surface_id } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Jj, "jj git fetch")?;
                let _output =
                    run_command(&target.repo_root, "jj", &["git", "fetch", "--all-remotes"])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
            VcsCommand::JjPush { surface_id } => {
                let target = self.resolve_target(model, surface_id)?;
                ensure_mode(target.mode, VcsMode::Jj, "jj git push")?;
                let _output = run_command(&target.repo_root, "jj", &["git", "push"])?;
                Ok(VcsCommandResult {
                    snapshot: Some(self.snapshot_from_target(&target, None)?),
                    message: None,
                })
            }
        }
    }

    fn snapshot_for_surface(
        &self,
        model: &AppModel,
        surface_id: SurfaceId,
        diff_path: Option<String>,
    ) -> Result<VcsSnapshot> {
        let target = self.resolve_target(model, surface_id)?;
        self.snapshot_from_target(&target, diff_path)
    }

    fn snapshot_from_target(
        &self,
        target: &RepoTarget,
        diff_path: Option<String>,
    ) -> Result<VcsSnapshot> {
        match target.mode {
            VcsMode::Git => self.git_snapshot(target, diff_path),
            VcsMode::Jj => self.jj_snapshot(target, diff_path),
        }
    }

    fn resolve_target(&self, model: &AppModel, surface_id: SurfaceId) -> Result<RepoTarget> {
        let surface = model
            .workspaces
            .values()
            .flat_map(|workspace| workspace.panes.values())
            .flat_map(|pane| pane.surfaces.values())
            .find(|surface| surface.id == surface_id)
            .ok_or_else(|| anyhow!("surface {surface_id} not found"))?;
        if surface.kind != PaneKind::Terminal {
            bail!("surface {surface_id} is not a terminal");
        }
        let cwd = surface
            .metadata
            .cwd
            .as_deref()
            .filter(|cwd| !cwd.trim().is_empty())
            .ok_or_else(|| anyhow!("terminal has no current working directory"))?;
        let cwd = PathBuf::from(cwd);
        let repo_root = resolve_git_root(&cwd)?;
        let mode = if repo_root.join(".jj").is_dir() {
            VcsMode::Jj
        } else {
            VcsMode::Git
        };
        Ok(RepoTarget {
            surface_id,
            cwd,
            repo_root,
            mode,
        })
    }

    fn git_snapshot(&self, target: &RepoTarget, diff_path: Option<String>) -> Result<VcsSnapshot> {
        let status = run_command(
            &target.repo_root,
            "git",
            &["status", "--porcelain=v2", "--branch"],
        )?;
        let git_status = parse_git_status(&status.stdout);
        let refs = parse_git_branches(
            &run_command(
                &target.repo_root,
                "git",
                &["branch", "--format=%(refname:short)|%(HEAD)"],
            )?
            .stdout,
        );
        let pull_request = github_pull_request(&target.repo_root, git_status.branch.as_deref())?;
        let diff_text = diff_path
            .as_deref()
            .map(|path| git_diff_preview(&target.repo_root, path))
            .transpose()?;
        Ok(VcsSnapshot {
            surface_id: target.surface_id,
            mode: VcsMode::Git,
            repo_root: target.repo_root.display().to_string(),
            repo_name: repo_name(&target.repo_root),
            cwd: target.cwd.display().to_string(),
            headline: git_status
                .branch
                .clone()
                .unwrap_or_else(|| "detached HEAD".into()),
            detail: if git_status.detached {
                git_status.head_oid.clone()
            } else {
                None
            },
            summary_text: git_summary_text(&git_status),
            files: git_status.files,
            refs,
            diff_path,
            diff_text,
            pull_request,
        })
    }

    fn jj_snapshot(&self, target: &RepoTarget, diff_path: Option<String>) -> Result<VcsSnapshot> {
        let status = run_command(&target.repo_root, "jj", &["status", "--color=never"])?;
        let current = run_command(
            &target.repo_root,
            "jj",
            &[
                "log",
                "-r",
                "@",
                "--no-graph",
                "-T",
                "change_id.short(8) ++ \"|\" ++ description.first_line() ++ \"|\" ++ bookmarks",
                "--color=never",
            ],
        )?;
        let current = parse_jj_current(&current.stdout);
        let refs = parse_jj_bookmarks(
            &run_command(
                &target.repo_root,
                "jj",
                &["bookmark", "list", "--color=never"],
            )?
            .stdout,
            current.bookmarks.as_slice(),
        );
        let files = parse_jj_diff_summary(
            &run_command(
                &target.repo_root,
                "jj",
                &["diff", "--summary", "--color=never"],
            )?
            .stdout,
        );
        let diff_text = diff_path
            .as_deref()
            .map(|path| {
                run_command(
                    &target.repo_root,
                    "jj",
                    &["diff", "--color=never", "--", path],
                )
                .map(|output| trim_output(&output.stdout, &output.stderr))
            })
            .transpose()?;
        let current_bookmark = current.bookmarks.first().cloned();
        let pull_request = github_pull_request(&target.repo_root, current_bookmark.as_deref())?;
        Ok(VcsSnapshot {
            surface_id: target.surface_id,
            mode: VcsMode::Jj,
            repo_root: target.repo_root.display().to_string(),
            repo_name: repo_name(&target.repo_root),
            cwd: target.cwd.display().to_string(),
            headline: current
                .bookmarks
                .first()
                .cloned()
                .unwrap_or_else(|| current.change_id.clone()),
            detail: Some(format!("{} · {}", current.change_id, current.description)),
            summary_text: trim_output(&status.stdout, &status.stderr),
            files,
            refs,
            diff_path,
            diff_text,
            pull_request,
        })
    }
}

#[derive(Debug, Default)]
struct ParsedGitStatus {
    branch: Option<String>,
    head_oid: Option<String>,
    detached: bool,
    files: Vec<VcsFileEntry>,
}

#[derive(Debug, Default)]
struct ParsedJjCurrent {
    change_id: String,
    description: String,
    bookmarks: Vec<String>,
}

fn ensure_mode(actual: VcsMode, expected: VcsMode, action: &str) -> Result<()> {
    if actual == expected {
        return Ok(());
    }
    bail!("{action} is unavailable in {:?} mode", actual);
}

fn ensure_non_empty(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(())
}

fn resolve_git_root(cwd: &Path) -> Result<PathBuf> {
    let output = run_command(cwd, "git", &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(output.stdout.trim()))
}

fn repo_name(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo")
        .to_string()
}

struct CommandOutput {
    stdout: String,
    stderr: String,
}

fn run_command(cwd: &Path, program: &str, args: &[&str]) -> Result<CommandOutput> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to run {program} {}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let detail = trim_output(&stdout, &stderr);
        bail!(
            "{} {} failed{}",
            program,
            args.join(" "),
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        );
    }
    Ok(CommandOutput { stdout, stderr })
}

fn trim_output(stdout: &str, stderr: &str) -> String {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    match (stdout.is_empty(), stderr.is_empty()) {
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) if stdout == stderr => stdout.to_string(),
        (false, false) => format!("{stdout}\n{stderr}"),
        (true, true) => String::new(),
    }
}

fn parse_git_status(raw: &str) -> ParsedGitStatus {
    let mut parsed = ParsedGitStatus::default();
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("# branch.head ") {
            if value == "(detached)" {
                parsed.detached = true;
            } else {
                parsed.branch = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("# branch.oid ") {
            if value != "(initial)" {
                parsed.head_oid = Some(value.chars().take(8).collect());
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("? ") {
            parsed.files.push(VcsFileEntry {
                path: path.to_string(),
                status: VcsFileStatus::Untracked,
                staged: false,
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("1 ") {
            let mut parts = rest.splitn(9, ' ');
            let xy = parts.next().unwrap_or("..");
            let path = rest
                .rsplit_once(' ')
                .map(|(_, path)| path)
                .unwrap_or_default()
                .to_string();
            if !path.is_empty() {
                parsed.files.extend(git_file_entries(path, xy, None));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("2 ") {
            let mut parts = rest.splitn(10, ' ');
            let xy = parts.next().unwrap_or("..");
            let paths = parts.nth(8).unwrap_or_default();
            let mut names = paths.split('\t');
            let original = names.next().unwrap_or_default();
            let path = names.next().unwrap_or(original).to_string();
            if !path.is_empty() {
                parsed
                    .files
                    .extend(git_file_entries(path, xy, Some(VcsFileStatus::Renamed)));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("u ") {
            let mut parts = rest.splitn(11, ' ');
            let _ = parts.next();
            let path = rest
                .rsplit_once(' ')
                .map(|(_, path)| path)
                .unwrap_or_default()
                .to_string();
            if !path.is_empty() {
                parsed.files.push(VcsFileEntry {
                    path,
                    status: VcsFileStatus::Conflicted,
                    staged: false,
                });
            }
        }
    }
    parsed
}

fn git_file_entries(
    path: String,
    xy: &str,
    override_status: Option<VcsFileStatus>,
) -> Vec<VcsFileEntry> {
    let chars: Vec<char> = xy.chars().collect();
    let index = chars.first().copied().unwrap_or('.');
    let worktree = chars.get(1).copied().unwrap_or('.');
    let mut entries = Vec::new();
    if index != '.' {
        entries.push(VcsFileEntry {
            path: path.clone(),
            status: override_status.unwrap_or_else(|| map_git_status(index)),
            staged: true,
        });
    }
    if worktree != '.' {
        entries.push(VcsFileEntry {
            path,
            status: override_status.unwrap_or_else(|| map_git_status(worktree)),
            staged: false,
        });
    }
    entries
}

fn map_git_status(value: char) -> VcsFileStatus {
    match value {
        'A' => VcsFileStatus::Added,
        'D' => VcsFileStatus::Deleted,
        'R' => VcsFileStatus::Renamed,
        'C' => VcsFileStatus::Copied,
        'U' => VcsFileStatus::Conflicted,
        'M' | 'T' => VcsFileStatus::Modified,
        _ => VcsFileStatus::Changed,
    }
}

fn parse_git_branches(raw: &str) -> Vec<VcsRefEntry> {
    raw.lines()
        .filter_map(|line| {
            let (name, head) = line.split_once('|')?;
            Some(VcsRefEntry {
                name: name.trim().to_string(),
                active: head.trim() == "*",
            })
        })
        .collect()
}

fn git_summary_text(status: &ParsedGitStatus) -> String {
    if status.files.is_empty() {
        match (&status.branch, status.detached) {
            (Some(branch), false) => format!("{branch} · working tree clean"),
            _ => "Working tree clean".into(),
        }
    } else {
        format!("{} file changes", status.files.len())
    }
}

fn git_diff_preview(repo_root: &Path, path: &str) -> Result<String> {
    let unstaged = run_command(repo_root, "git", &["diff", "--no-ext-diff", "--", path])
        .map(|output| output.stdout)
        .unwrap_or_default();
    let staged = run_command(
        repo_root,
        "git",
        &["diff", "--no-ext-diff", "--cached", "--", path],
    )
    .map(|output| output.stdout)
    .unwrap_or_default();
    let combined = [staged.trim(), unstaged.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(combined)
}

fn parse_jj_current(raw: &str) -> ParsedJjCurrent {
    let mut parts = raw.trim().splitn(3, '|');
    let change_id = parts.next().unwrap_or_default().trim().to_string();
    let description = parts.next().unwrap_or_default().trim().to_string();
    let bookmarks = parts
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    ParsedJjCurrent {
        change_id,
        description,
        bookmarks,
    }
}

fn parse_jj_bookmarks(raw: &str, active: &[String]) -> Vec<VcsRefEntry> {
    raw.lines()
        .filter_map(|line| {
            let (name, _) = line.split_once(':')?;
            let name = name.trim();
            (!name.is_empty()).then(|| VcsRefEntry {
                name: name.to_string(),
                active: active.iter().any(|bookmark| bookmark == name),
            })
        })
        .collect()
}

fn parse_jj_diff_summary(raw: &str) -> Vec<VcsFileEntry> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let mut parts = line.split_whitespace();
            let status = parts.next().unwrap_or_default();
            let path = parts.collect::<Vec<_>>().join(" ");
            if path.is_empty() {
                return None;
            }
            Some(VcsFileEntry {
                path,
                status: match status {
                    "A" => VcsFileStatus::Added,
                    "D" => VcsFileStatus::Deleted,
                    "M" => VcsFileStatus::Modified,
                    "R" => VcsFileStatus::Renamed,
                    "C" => VcsFileStatus::Copied,
                    _ => VcsFileStatus::Changed,
                },
                staged: false,
            })
        })
        .collect()
}

fn github_pull_request(repo_root: &Path, head: Option<&str>) -> Result<Option<VcsPullRequestInfo>> {
    let Some(head) = head.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if !command_exists("gh") {
        return Ok(None);
    }
    let origin = run_command(repo_root, "git", &["remote", "get-url", "origin"])
        .map(|output| output.stdout)
        .unwrap_or_default();
    if !origin.contains("github.com") {
        return Ok(None);
    }
    let output = Command::new("gh")
        .args(["pr", "view", head, "--json", "number,title,url,state"])
        .current_dir(repo_root)
        .output()
        .context("failed to run gh pr view")?;
    if !output.status.success() {
        return Ok(None);
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse gh pr view json")?;
    Ok(Some(VcsPullRequestInfo {
        number: value
            .get("number")
            .and_then(|value| value.as_u64())
            .map(|value| value as u32),
        title: value
            .get("title")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        url: value
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        state: value
            .get("state")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }))
}

fn command_exists(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{parse_git_status, parse_jj_bookmarks, parse_jj_current};
    use taskers_control::VcsFileStatus;

    #[test]
    fn parses_git_porcelain_v2_changes() {
        let parsed = parse_git_status(
            "# branch.oid 1234567890\n# branch.head main\n1 M. N... 100644 100644 100644 abc abc file.txt\n? new.rs\nu UU N... 100644 100644 100644 100644 abc abc abc conflict.rs\n",
        );
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.files.len(), 3);
        assert_eq!(parsed.files[0].status, VcsFileStatus::Modified);
        assert!(parsed.files[0].staged);
        assert_eq!(parsed.files[1].status, VcsFileStatus::Untracked);
        assert_eq!(parsed.files[2].status, VcsFileStatus::Conflicted);
    }

    #[test]
    fn parses_jj_current_and_bookmarks() {
        let current = parse_jj_current("abcd1234|feat: title|main feature-x\n");
        assert_eq!(current.change_id, "abcd1234");
        assert_eq!(current.description, "feat: title");
        assert_eq!(current.bookmarks, vec!["main", "feature-x"]);

        let bookmarks = parse_jj_bookmarks(
            "main: xyz 123 base\nfeature-x: xyz 456 work\n",
            &current.bookmarks,
        );
        assert_eq!(bookmarks.len(), 2);
        assert!(bookmarks[0].active);
        assert!(bookmarks[1].active);
    }
}
