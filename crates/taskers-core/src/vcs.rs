use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use taskers_control::{
    VcsCommand, VcsCommandResult, VcsCommitEntry, VcsFileEntry, VcsFileStatus, VcsMode,
    VcsPullRequestInfo, VcsRefEntry, VcsSnapshot,
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
        let (repo_root, mode) = resolve_repo_root(&cwd)?;
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
        let summary_text = git_summary_text(&git_status);
        let unstaged_stats = parse_git_numstat(
            &run_command(&target.repo_root, "git", &["diff", "--numstat"])
                .map(|o| o.stdout)
                .unwrap_or_default(),
        );
        let staged_stats = parse_git_numstat(
            &run_command(&target.repo_root, "git", &["diff", "--numstat", "--cached"])
                .map(|o| o.stdout)
                .unwrap_or_default(),
        );
        let mut files = git_status.files;
        let (total_insertions, total_deletions) =
            enrich_git_files_with_stats(&mut files, &staged_stats, &unstaged_stats);
        // Try upstream..HEAD first, fall back to origin/HEAD..HEAD, then empty
        let commits_raw = run_command(
            &target.repo_root,
            "git",
            &["log", "--format=%h\t%s", "--shortstat", "@{upstream}..HEAD"],
        )
        .or_else(|_| {
            run_command(
                &target.repo_root,
                "git",
                &["log", "--format=%h\t%s", "--shortstat", "origin/HEAD..HEAD"],
            )
        })
        .map(|o| o.stdout)
        .unwrap_or_default();
        let recent_commits = parse_git_log_shortstat(&commits_raw);
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
            summary_text,
            files,
            refs,
            diff_path,
            diff_text,
            pull_request,
            total_insertions,
            total_deletions,
            recent_commits,
        })
    }

    fn jj_snapshot(&self, target: &RepoTarget, diff_path: Option<String>) -> Result<VcsSnapshot> {
        let status = run_command(&target.repo_root, "jj", &["status", "--color=never"])?;
        let current = load_jj_current(&target.repo_root)?;
        let refs = parse_jj_bookmarks(
            &run_command(
                &target.repo_root,
                "jj",
                &["bookmark", "list", "--color=never"],
            )?
            .stdout,
            current.bookmarks.as_slice(),
        );
        let mut files = parse_jj_diff_summary(
            &run_command(
                &target.repo_root,
                "jj",
                &["diff", "--summary", "--color=never"],
            )?
            .stdout,
        );
        let stat_output = run_command(
            &target.repo_root,
            "jj",
            &["diff", "--stat", "--color=never"],
        )
        .map(|o| o.stdout)
        .unwrap_or_default();
        let stat_map = parse_jj_diff_stat(&stat_output);
        let (total_insertions, total_deletions) = enrich_files_with_stats(&mut files, &stat_map);
        let diff_text = diff_path
            .as_deref()
            .map(|path| jj_diff_preview(&target.repo_root, path));
        let current_bookmark = current.bookmarks.first().cloned();
        let pull_request = github_pull_request(&target.repo_root, current_bookmark.as_deref())?;
        let recent_commits = parse_jj_log_stat(
            &run_command(
                &target.repo_root,
                "jj",
                &[
                    "log", "--no-graph", "--color=never", "--stat",
                    "-T", r#"change_id.short(8) ++ "\t" ++ if(description, description.first_line(), "(no description)") ++ "\n""#,
                    "-r", "remote_bookmarks()..@",
                ],
            )
            .map(|o| o.stdout)
            .unwrap_or_default(),
        );
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
            total_insertions,
            total_deletions,
            recent_commits,
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

fn resolve_repo_root(cwd: &Path) -> Result<(PathBuf, VcsMode)> {
    for ancestor in cwd.ancestors() {
        let has_jj = ancestor.join(".jj").is_dir();
        let git_marker = ancestor.join(".git");
        let has_git = git_marker.is_dir() || git_marker.is_file();

        if has_jj {
            return Ok((ancestor.to_path_buf(), VcsMode::Jj));
        }

        if has_git {
            return Ok((ancestor.to_path_buf(), VcsMode::Git));
        }
    }

    bail!("no git or jj repository found from {}", cwd.display())
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

fn load_jj_current(repo_root: &Path) -> Result<ParsedJjCurrent> {
    let change_id = run_command(
        repo_root,
        "jj",
        &[
            "log",
            "-r",
            "@",
            "--no-graph",
            "-T",
            "change_id.short(8)",
            "--color=never",
        ],
    )?;
    let description = run_command(
        repo_root,
        "jj",
        &[
            "log",
            "-r",
            "@",
            "--no-graph",
            "-T",
            "description.first_line()",
            "--color=never",
        ],
    )?;
    let bookmarks = run_command(
        repo_root,
        "jj",
        &[
            "log",
            "-r",
            "@",
            "--no-graph",
            "-T",
            r#"bookmarks.join("\n")"#,
            "--color=never",
        ],
    )?;
    Ok(parse_jj_current(
        &change_id.stdout,
        &description.stdout,
        &bookmarks.stdout,
    ))
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
                insertions: None,
                deletions: None,
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("1 ") {
            let mut parts = rest.splitn(8, ' ');
            let xy = parts.next().unwrap_or("..");
            let path = parts.nth(6).unwrap_or_default().to_string();
            if !path.is_empty() {
                parsed.files.extend(git_file_entries(path, xy, None));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("2 ") {
            let mut parts = rest.splitn(9, ' ');
            let xy = parts.next().unwrap_or("..");
            let paths = parts.nth(7).unwrap_or_default();
            let mut names = paths.split('\t');
            let path = names.next().unwrap_or_default().to_string();
            if !path.is_empty() {
                parsed.files.extend(git_file_entries(path, xy, None));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("u ") {
            let mut parts = rest.splitn(10, ' ');
            let _ = parts.next();
            let path = parts.nth(8).unwrap_or_default().to_string();
            if !path.is_empty() {
                parsed.files.push(VcsFileEntry {
                    path,
                    status: VcsFileStatus::Conflicted,
                    staged: false,
                    insertions: None,
                    deletions: None,
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
            insertions: None,
            deletions: None,
        });
    }
    if worktree != '.' {
        entries.push(VcsFileEntry {
            path,
            status: override_status.unwrap_or_else(|| map_git_status(worktree)),
            staged: false,
            insertions: None,
            deletions: None,
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

fn jj_diff_preview(repo_root: &Path, path: &str) -> String {
    run_command(repo_root, "jj", &["diff", "--color=never", "--", path])
        .map(|output| trim_output(&output.stdout, &output.stderr))
        .unwrap_or_default()
}

fn parse_jj_current(
    change_id_raw: &str,
    description_raw: &str,
    bookmarks_raw: &str,
) -> ParsedJjCurrent {
    let change_id = change_id_raw.trim().to_string();
    let description = description_raw.trim().to_string();
    let bookmarks = bookmarks_raw
        .lines()
        .map(str::trim)
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
            if line.trim().is_empty() {
                return None;
            }
            let separator = line.char_indices().find(|(_, ch)| ch.is_whitespace())?.0;
            let status = line[..separator].trim();
            let path = line[separator..]
                .trim_start_matches(char::is_whitespace)
                .to_string();
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
                insertions: None,
                deletions: None,
            })
        })
        .collect()
}

fn parse_jj_diff_stat(raw: &str) -> HashMap<String, (u32, u32)> {
    let mut stats = HashMap::new();
    for line in raw.lines() {
        let Some((left, right)) = line.split_once('|') else {
            continue;
        };
        let path = left.trim().to_string();
        if path.is_empty() {
            continue;
        }
        let right = right.trim();
        // First token after | is the total change count
        let count: u32 = match right.split_whitespace().next().and_then(|s| s.parse().ok()) {
            Some(n) => n,
            None => continue, // summary line or unparseable
        };
        let plus_count = right.chars().filter(|&c| c == '+').count() as u32;
        let minus_count = right.chars().filter(|&c| c == '-').count() as u32;
        let total_chars = plus_count + minus_count;
        if total_chars == 0 {
            continue;
        }
        let insertions = count * plus_count / total_chars;
        let deletions = count - insertions;
        stats.insert(path, (insertions, deletions));
    }
    stats
}

fn parse_git_numstat(raw: &str) -> HashMap<String, (u32, u32)> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let ins: u32 = parts.next()?.parse().ok()?;
            let del: u32 = parts.next()?.parse().ok()?;
            let path = parts.next()?.to_string();
            Some((path, (ins, del)))
        })
        .collect()
}

fn enrich_files_with_stats(
    files: &mut [VcsFileEntry],
    stats: &HashMap<String, (u32, u32)>,
) -> (u32, u32) {
    let mut total_ins = 0u32;
    let mut total_del = 0u32;
    for file in files.iter_mut() {
        if let Some(&(ins, del)) = stats.get(&file.path) {
            file.insertions = Some(ins);
            file.deletions = Some(del);
            total_ins += ins;
            total_del += del;
        }
    }
    (total_ins, total_del)
}

fn enrich_git_files_with_stats(
    files: &mut [VcsFileEntry],
    staged_stats: &HashMap<String, (u32, u32)>,
    unstaged_stats: &HashMap<String, (u32, u32)>,
) -> (u32, u32) {
    for file in files.iter_mut() {
        let stats = if file.staged {
            staged_stats.get(&file.path)
        } else {
            unstaged_stats.get(&file.path)
        };
        if let Some(&(ins, del)) = stats {
            file.insertions = Some(ins);
            file.deletions = Some(del);
        }
    }
    let total_ins = staged_stats
        .values()
        .chain(unstaged_stats.values())
        .map(|(ins, _)| ins)
        .copied()
        .sum();
    let total_del = staged_stats
        .values()
        .chain(unstaged_stats.values())
        .map(|(_, del)| del)
        .copied()
        .sum();
    (total_ins, total_del)
}

/// Parse `git log --format="%h%x09%s" --shortstat -n N` output.
/// Lines alternate between "hash\tdescription" and "N files changed, X insertions(+), Y deletions(-)".
fn parse_git_log_shortstat(raw: &str) -> Vec<VcsCommitEntry> {
    let mut commits = Vec::new();
    let mut current_id = String::new();
    let mut current_desc = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((hash, desc)) = trimmed.split_once('\t') {
            // Flush previous commit if pending
            if !current_id.is_empty() {
                commits.push(VcsCommitEntry {
                    id: std::mem::take(&mut current_id),
                    description: std::mem::take(&mut current_desc),
                    insertions: 0,
                    deletions: 0,
                });
            }
            current_id = hash.to_string();
            current_desc = desc.to_string();
        } else if trimmed.contains("changed") {
            // Stat summary line: "N file(s) changed, X insertion(s)(+), Y deletion(s)(-)"
            let (ins, del) = parse_shortstat_line(trimmed);
            commits.push(VcsCommitEntry {
                id: std::mem::take(&mut current_id),
                description: std::mem::take(&mut current_desc),
                insertions: ins,
                deletions: del,
            });
        }
    }
    // Flush last commit without stats
    if !current_id.is_empty() {
        commits.push(VcsCommitEntry {
            id: current_id,
            description: current_desc,
            insertions: 0,
            deletions: 0,
        });
    }
    commits
}

/// Parse `jj log --no-graph --color=never --stat -T 'template'` output.
/// Template produces "change_id\tdescription" lines interleaved with stat output.
fn parse_jj_log_stat(raw: &str) -> Vec<VcsCommitEntry> {
    let mut commits = Vec::new();
    let mut current_id = String::new();
    let mut current_desc = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((id, desc)) = trimmed.split_once('\t') {
            // If the "id" part looks like a short change ID (alphanumeric, no spaces, no |)
            if !id.is_empty() && !id.contains('|') && !id.contains("changed") && id.len() <= 16 {
                // Flush previous
                if !current_id.is_empty() {
                    commits.push(VcsCommitEntry {
                        id: std::mem::take(&mut current_id),
                        description: std::mem::take(&mut current_desc),
                        insertions: 0,
                        deletions: 0,
                    });
                }
                current_id = id.to_string();
                current_desc = desc.to_string();
                continue;
            }
        }
        if trimmed.contains("changed")
            && (trimmed.contains("insertion") || trimmed.contains("deletion"))
        {
            let (ins, del) = parse_shortstat_line(trimmed);
            commits.push(VcsCommitEntry {
                id: std::mem::take(&mut current_id),
                description: std::mem::take(&mut current_desc),
                insertions: ins,
                deletions: del,
            });
        }
        // Skip per-file stat lines (contain |)
    }
    if !current_id.is_empty() {
        commits.push(VcsCommitEntry {
            id: current_id,
            description: current_desc,
            insertions: 0,
            deletions: 0,
        });
    }
    commits
}

/// Extract insertions/deletions from a shortstat summary line like
/// "3 files changed, 10 insertions(+), 5 deletions(-)"
fn parse_shortstat_line(line: &str) -> (u32, u32) {
    let mut ins = 0u32;
    let mut del = 0u32;
    let words: Vec<&str> = line.split_whitespace().collect();
    for window in words.windows(2) {
        if window[1].starts_with("insertion") {
            ins = window[0].parse().unwrap_or(0);
        } else if window[1].starts_with("deletion") {
            del = window[0].parse().unwrap_or(0);
        }
    }
    (ins, del)
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
    use std::{collections::HashMap, fs};

    use super::{
        enrich_files_with_stats, enrich_git_files_with_stats, jj_diff_preview,
        parse_git_log_shortstat, parse_git_numstat, parse_git_status, parse_jj_bookmarks,
        parse_jj_current, parse_jj_diff_stat, parse_jj_diff_summary, parse_jj_log_stat,
        resolve_repo_root,
    };
    use taskers_control::{VcsFileEntry, VcsFileStatus, VcsMode};
    use tempfile::TempDir;

    #[test]
    fn parses_git_porcelain_v2_changes() {
        let parsed = parse_git_status(
            "# branch.oid 1234567890\n# branch.head main\n1 M. N... 100644 100644 100644 abc abc file with spaces.txt\n2 RM N... 100644 100644 100644 abc def R100 renamed file.txt\toriginal file.txt\n? new.rs\nu UU N... 100644 100644 100644 100644 abc abc abc conflict file.rs\n",
        );
        assert_eq!(parsed.branch.as_deref(), Some("main"));
        assert_eq!(parsed.files.len(), 5);
        assert_eq!(parsed.files[0].path, "file with spaces.txt");
        assert_eq!(parsed.files[0].status, VcsFileStatus::Modified);
        assert!(parsed.files[0].staged);
        assert_eq!(parsed.files[1].path, "renamed file.txt");
        assert_eq!(parsed.files[1].status, VcsFileStatus::Renamed);
        assert!(parsed.files[1].staged);
        assert_eq!(parsed.files[2].path, "renamed file.txt");
        assert_eq!(parsed.files[2].status, VcsFileStatus::Modified);
        assert!(!parsed.files[2].staged);
        assert_eq!(parsed.files[3].status, VcsFileStatus::Untracked);
        assert_eq!(parsed.files[4].path, "conflict file.rs");
        assert_eq!(parsed.files[4].status, VcsFileStatus::Conflicted);
    }

    #[test]
    fn parses_jj_current_and_bookmarks() {
        let current = parse_jj_current(
            "abcd1234\n",
            "feat: title | with pipe\n",
            "main\nfeature-x\n",
        );
        assert_eq!(current.change_id, "abcd1234");
        assert_eq!(current.description, "feat: title | with pipe");
        assert_eq!(current.bookmarks, vec!["main", "feature-x"]);

        let bookmarks = parse_jj_bookmarks(
            "main: xyz 123 base\nfeature-x: xyz 456 work\n",
            &current.bookmarks,
        );
        assert_eq!(bookmarks.len(), 2);
        assert!(bookmarks[0].active);
        assert!(bookmarks[1].active);
    }

    #[test]
    fn parses_jj_diff_summary_without_normalizing_whitespace() {
        let parsed = parse_jj_diff_summary("M a  b.txt\nA tab\tname.txt\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].path, "a  b.txt");
        assert_eq!(parsed[0].status, VcsFileStatus::Modified);
        assert_eq!(parsed[1].path, "tab\tname.txt");
        assert_eq!(parsed[1].status, VcsFileStatus::Added);
    }

    #[test]
    fn resolves_jj_repo_root_without_git_metadata() {
        let temp = TempDir::new().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let cwd = repo_root.join("nested/work");
        fs::create_dir_all(repo_root.join(".jj")).expect("jj dir");
        fs::create_dir_all(&cwd).expect("cwd");

        let (resolved_root, mode) = resolve_repo_root(&cwd).expect("resolve repo root");
        assert_eq!(resolved_root, repo_root);
        assert_eq!(mode, VcsMode::Jj);
    }

    #[test]
    fn prefers_same_root_jj_marker_over_git_marker() {
        let temp = TempDir::new().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let cwd = repo_root.join("nested/work");
        fs::create_dir_all(repo_root.join(".jj")).expect("jj dir");
        fs::create_dir_all(repo_root.join(".git")).expect("git dir");
        fs::create_dir_all(&cwd).expect("cwd");

        let (resolved_root, mode) = resolve_repo_root(&cwd).expect("resolve repo root");
        assert_eq!(resolved_root, repo_root);
        assert_eq!(mode, VcsMode::Jj);
    }

    #[test]
    fn prefers_nearest_git_marker_over_parent_jj_repo() {
        let temp = TempDir::new().expect("tempdir");
        let outer_root = temp.path().join("outer");
        let git_root = outer_root.join("nested-git");
        let cwd = git_root.join("src");
        fs::create_dir_all(outer_root.join(".jj")).expect("outer jj dir");
        fs::create_dir_all(git_root.join(".git")).expect("git dir");
        fs::create_dir_all(&cwd).expect("cwd");

        let (resolved_root, mode) = resolve_repo_root(&cwd).expect("resolve repo root");
        assert_eq!(resolved_root, git_root);
        assert_eq!(mode, VcsMode::Git);
    }

    #[test]
    fn jj_diff_preview_returns_empty_when_preview_fails() {
        let temp = TempDir::new().expect("tempdir");
        assert_eq!(jj_diff_preview(temp.path(), "missing.txt"), "");
    }

    #[test]
    fn parses_jj_diff_stat_output() {
        let raw = "src/main.rs  | 12 ++++++------\nsrc/lib.rs   |  4 ++++\n2 files changed, 10 insertions(+), 6 deletions(-)\n";
        let stats = parse_jj_diff_stat(raw);
        assert_eq!(stats.len(), 2);
        let (ins, del) = stats["src/main.rs"];
        assert_eq!(ins, 6);
        assert_eq!(del, 6);
        let (ins, del) = stats["src/lib.rs"];
        assert_eq!(ins, 4);
        assert_eq!(del, 0);
    }

    #[test]
    fn parses_git_numstat_output() {
        let raw = "10\t5\tsrc/main.rs\n3\t0\tREADME.md\n-\t-\tbinary.png\n";
        let stats = parse_git_numstat(raw);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats["src/main.rs"], (10, 5));
        assert_eq!(stats["README.md"], (3, 0));
        assert!(!stats.contains_key("binary.png"));
    }

    #[test]
    fn parses_git_log_shortstat() {
        let raw = "abc1234\tfix: handle edge case\n\n 3 files changed, 10 insertions(+), 5 deletions(-)\n\ndef5678\tfeat: add new feature\n\n 1 file changed, 20 insertions(+)\n";
        let commits = parse_git_log_shortstat(raw);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "abc1234");
        assert_eq!(commits[0].description, "fix: handle edge case");
        assert_eq!(commits[0].insertions, 10);
        assert_eq!(commits[0].deletions, 5);
        assert_eq!(commits[1].id, "def5678");
        assert_eq!(commits[1].insertions, 20);
        assert_eq!(commits[1].deletions, 0);
    }

    #[test]
    fn parses_git_log_shortstat_with_statless_commit_before_next_entry() {
        let raw = "abc1234\tchore: empty change\n\ndef5678\tfeat: add widget\n\n 1 file changed, 2 insertions(+), 1 deletion(-)\n";
        let commits = parse_git_log_shortstat(raw);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "abc1234");
        assert_eq!(commits[0].insertions, 0);
        assert_eq!(commits[0].deletions, 0);
        assert_eq!(commits[1].id, "def5678");
        assert_eq!(commits[1].insertions, 2);
        assert_eq!(commits[1].deletions, 1);
    }

    #[test]
    fn parses_jj_log_stat() {
        let raw = "abcd1234\tfix: centralize state\nsrc/main.rs | 12 ++++++------\nsrc/lib.rs  |  4 ++++\n2 files changed, 10 insertions(+), 6 deletions(-)\nefgh5678\tfeat: add widget\nwidget.rs | 30 ++++++++++++++++++++++++++++++\n1 file changed, 30 insertions(+), 0 deletions(-)\n";
        let commits = parse_jj_log_stat(raw);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "abcd1234");
        assert_eq!(commits[0].insertions, 10);
        assert_eq!(commits[0].deletions, 6);
        assert_eq!(commits[1].id, "efgh5678");
        assert_eq!(commits[1].insertions, 30);
        assert_eq!(commits[1].deletions, 0);
    }

    #[test]
    fn enriches_matching_files_with_stats_and_totals() {
        let mut files = vec![
            VcsFileEntry {
                path: "src/main.rs".into(),
                status: VcsFileStatus::Modified,
                staged: false,
                insertions: None,
                deletions: None,
            },
            VcsFileEntry {
                path: "README.md".into(),
                status: VcsFileStatus::Added,
                staged: true,
                insertions: None,
                deletions: None,
            },
        ];
        let stats = HashMap::from([
            ("src/main.rs".to_string(), (5, 2)),
            ("ignored.rs".to_string(), (9, 9)),
        ]);

        let (total_ins, total_del) = enrich_files_with_stats(&mut files, &stats);

        assert_eq!((total_ins, total_del), (5, 2));
        assert_eq!(files[0].insertions, Some(5));
        assert_eq!(files[0].deletions, Some(2));
        assert_eq!(files[1].insertions, None);
        assert_eq!(files[1].deletions, None);
    }

    #[test]
    fn enriches_git_stats_without_double_counting_split_entries() {
        let mut files = vec![
            VcsFileEntry {
                path: "src/main.rs".into(),
                status: VcsFileStatus::Modified,
                staged: true,
                insertions: None,
                deletions: None,
            },
            VcsFileEntry {
                path: "src/main.rs".into(),
                status: VcsFileStatus::Modified,
                staged: false,
                insertions: None,
                deletions: None,
            },
            VcsFileEntry {
                path: "README.md".into(),
                status: VcsFileStatus::Added,
                staged: false,
                insertions: None,
                deletions: None,
            },
        ];
        let staged_stats = HashMap::from([("src/main.rs".to_string(), (2, 1))]);
        let unstaged_stats = HashMap::from([
            ("src/main.rs".to_string(), (3, 0)),
            ("README.md".to_string(), (4, 0)),
        ]);

        let (total_ins, total_del) =
            enrich_git_files_with_stats(&mut files, &staged_stats, &unstaged_stats);

        assert_eq!((total_ins, total_del), (9, 1));
        assert_eq!(files[0].insertions, Some(2));
        assert_eq!(files[0].deletions, Some(1));
        assert_eq!(files[1].insertions, Some(3));
        assert_eq!(files[1].deletions, Some(0));
        assert_eq!(files[2].insertions, Some(4));
        assert_eq!(files[2].deletions, Some(0));
    }
}
