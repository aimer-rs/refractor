//! Git repository operations implemented with `git2`.

use git2::{Cred, CredentialType, PushOptions, RemoteCallbacks, Repository};
use std::path::{Path, PathBuf};

use super::Change;

/// A file changed in the repository workdir or index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    /// Relative path to repository root.
    pub path: PathBuf,
    /// Absolute path on disk.
    pub absolute_path: PathBuf,
    /// What kind of change git detects.
    pub change: Change,
    /// Whether the file is already staged in the git index.
    pub staged: bool,
}

/// A short summary of a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitSummary {
    /// Full 40-character hex commit id.
    pub id: String,
    /// 7-character prefix.
    pub short_id: String,
    /// Commit subject line.
    pub message: String,
    /// Author name.
    pub author: String,
    /// Timestamp in epoch seconds.
    pub time_seconds: i64,
}

/// Information about commits waiting to be pushed to a remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushInfo {
    /// Current checked out branch name.
    pub branch: String,
    /// Remote name (e.g. `"origin"`).
    pub remote: String,
    /// Target remote tracking branch (e.g. `"origin/main"`).
    pub remote_branch: String,
    /// Commits ahead of upstream in newest-first order.
    pub commits: Vec<CommitSummary>,
}

/// Discovers the git repository containing `path`.
pub fn open_repo(path: &Path) -> Result<Repository, String> {
    Repository::discover(path).map_err(|e| e.to_string())
}

/// Lists files with uncommitted changes in `dir`.
pub fn changed_files(dir: &Path) -> Result<Vec<ChangedFile>, String> {
    let repo = open_repo(dir)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| "bare repository".to_string())?;
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);

    let statuses = repo
        .statuses(Some(&mut options))
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();

    for entry in statuses.iter() {
        let Some(rel_str) = entry.path() else {
            continue;
        };
        let rel_path = PathBuf::from(rel_str);
        let status = entry.status();

        let staged = status.intersects(
            git2::Status::INDEX_NEW
                | git2::Status::INDEX_MODIFIED
                | git2::Status::INDEX_DELETED
                | git2::Status::INDEX_RENAMED
                | git2::Status::INDEX_TYPECHANGE,
        );

        let change = if status.contains(git2::Status::CONFLICTED) {
            Change::Conflict
        } else if status.contains(git2::Status::WT_NEW) {
            Change::Untracked
        } else if status.contains(git2::Status::INDEX_NEW) {
            Change::Added
        } else if status.intersects(git2::Status::WT_DELETED | git2::Status::INDEX_DELETED) {
            Change::Deleted
        } else if status.intersects(git2::Status::WT_RENAMED | git2::Status::INDEX_RENAMED) {
            Change::Renamed
        } else {
            Change::Modified
        };

        result.push(ChangedFile {
            absolute_path: workdir.join(&rel_path),
            path: rel_path,
            change,
            staged,
        });
    }

    Ok(result)
}

/// Commits the specified `files` to `dir`'s repository with `message`.
///
/// Only `files` will be included in the commit. Any existing uncommitted changes in other files
/// remain untouched in the working directory.
pub fn commit(dir: &Path, files: &[PathBuf], message: &str) -> Result<String, String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err("Commit message cannot be empty".to_string());
    }
    if files.is_empty() {
        return Err("No files selected to commit".to_string());
    }

    let repo = open_repo(dir)?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| "bare repository".to_string())?;
    let mut index = repo.index().map_err(|e| e.to_string())?;

    // Reset index to HEAD tree first so only `files` will be staged into this commit.
    if let Ok(head_commit) = repo.head().and_then(|h| h.peel_to_commit()) {
        let head_tree = head_commit.tree().map_err(|e| e.to_string())?;
        index
            .read_tree(&head_tree)
            .map_err(|e| e.to_string())?;
    } else {
        index.clear().map_err(|e| e.to_string())?;
    }

    for file in files {
        let rel_path = if file.is_absolute() {
            file.strip_prefix(workdir)
                .map_err(|e| e.to_string())?
        } else {
            file.as_path()
        };
        let full = workdir.join(rel_path);
        if full.exists() {
            index
                .add_path(rel_path)
                .map_err(|e| e.to_string())?;
        } else {
            index
                .remove_path(rel_path)
                .map_err(|e| e.to_string())?;
        }
    }

    index.write().map_err(|e| e.to_string())?;
    let tree_id = index.write_tree().map_err(|e| e.to_string())?;
    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| e.to_string())?;

    let sig = repo
        .signature()
        .or_else(|_| git2::Signature::now("Refractor User", "refractor@aimer.dev"))
        .map_err(|e| e.to_string())?;

    let parent_commit = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok());
    let parents = match &parent_commit {
        Some(p) => vec![p],
        None => vec![],
    };

    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, trimmed, &tree, &parents)
        .map_err(|e| e.to_string())?;

    Ok(oid.to_string())
}

/// Inspects the current branch and returns information about commits waiting to be pushed.
pub fn unpushed_info(dir: &Path) -> Result<PushInfo, String> {
    let repo = open_repo(dir)?;
    let head = repo.head().map_err(|e| e.to_string())?;
    let branch_name = head
        .shorthand()
        .ok_or_else(|| "detached HEAD".to_string())?
        .to_string();
    let local_oid = head
        .target()
        .ok_or_else(|| "HEAD has no target".to_string())?;

    let branch_ref = head.name().unwrap_or("refs/heads/main");
    let remote_name = repo
        .branch_upstream_remote(branch_ref)
        .ok()
        .and_then(|b| b.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "origin".to_string());

    let upstream_ref_name = repo
        .branch_upstream_name(branch_ref)
        .ok()
        .and_then(|b| b.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| format!("refs/remotes/{remote_name}/{branch_name}"));

    let upstream_oid = repo
        .find_reference(&upstream_ref_name)
        .ok()
        .and_then(|r| r.target());

    let mut revwalk = repo.revwalk().map_err(|e| e.to_string())?;
    revwalk
        .push(local_oid)
        .map_err(|e| e.to_string())?;
    if let Some(up_oid) = upstream_oid {
        let _ = revwalk.hide(up_oid);
    }

    let mut commits = Vec::new();
    for oid_res in revwalk {
        let oid = match oid_res {
            Ok(oid) => oid,
            Err(_) => break,
        };
        if Some(oid) == upstream_oid {
            break;
        }
        if let Ok(c) = repo.find_commit(oid) {
            let full_id = oid.to_string();
            let short_id = full_id[..7.min(full_id.len())].to_string();
            commits.push(CommitSummary {
                id: full_id,
                short_id,
                message: c.summary().unwrap_or("").to_string(),
                author: c.author().name().unwrap_or("").to_string(),
                time_seconds: c.time().seconds(),
            });
        }
    }

    Ok(PushInfo {
        remote_branch: format!("{remote_name}/{branch_name}"),
        branch: branch_name,
        remote: remote_name,
        commits,
    })
}

/// Pushes `branch` to `remote` using default/configured credentials.
pub fn push(
    dir: &Path,
    remote_name: Option<&str>,
    branch_name: Option<&str>,
) -> Result<(), String> {
    let repo = open_repo(dir)?;
    let head = repo.head().map_err(|e| e.to_string())?;

    let branch = match branch_name {
        Some(b) => b.to_string(),
        None => head
            .shorthand()
            .ok_or_else(|| "detached HEAD".to_string())?
            .to_string(),
    };

    let remote_target = match remote_name {
        Some(r) => r.to_string(),
        None => {
            let branch_ref = head.name().unwrap_or("refs/heads/main");
            repo.branch_upstream_remote(branch_ref)
                .ok()
                .and_then(|b| b.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "origin".to_string())
        }
    };

    let mut remote = repo
        .find_remote(&remote_target)
        .map_err(|e| e.to_string())?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, allowed_types| {
        if allowed_types.contains(CredentialType::SSH_KEY) {
            let user = username_from_url.unwrap_or("git");
            if let Ok(cred) = Cred::ssh_key_from_agent(user) {
                return Ok(cred);
            }
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                let ssh_dir = home.join(".ssh");
                for key_name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
                    let priv_key = ssh_dir.join(key_name);
                    if priv_key.is_file() {
                        let pub_key = ssh_dir.join(format!("{key_name}.pub"));
                        let pub_path = pub_key.is_file().then_some(pub_key.as_path());
                        if let Ok(cred) = Cred::ssh_key(user, pub_path, &priv_key, None) {
                            return Ok(cred);
                        }
                    }
                }
            }
        }
        if allowed_types.contains(CredentialType::USER_PASS_PLAINTEXT)
            && let Ok(config) = git2::Config::open_default()
            && let Ok(cred) = Cred::credential_helper(&config, _url, username_from_url)
        {
            return Ok(cred);
        }
        if allowed_types.contains(CredentialType::DEFAULT) {
            return Cred::default();
        }
        Cred::default()
    });

    let mut push_opts = PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    if let Err(git2_err) = remote.push(&[&refspec], Some(&mut push_opts)) {
        // Fallback to the system git CLI when libgit2's built-in transport cannot handle
        // the remote protocol (e.g. custom SSH agent/config in a staticlib build).
        let workdir = repo.workdir().unwrap_or(dir);
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(workdir)
            .args(["push", "-u", &remote_target, &branch])
            .output()
            .map_err(|_| git2_err.to_string())?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr)
                .trim()
                .to_owned();
            return Err(if stderr.is_empty() {
                git2_err.to_string()
            } else {
                stderr
            });
        }
    }

    // Attempt to set upstream if not already set
    if let Ok(mut local_branch) = repo.find_branch(&branch, git2::BranchType::Local)
        && local_branch.upstream().is_err()
    {
        let _ = local_branch.set_upstream(Some(&format!("{remote_target}/{branch}")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tree::Scratch;

    fn setup_test_repo(name: &str) -> (Scratch, Repository) {
        let scratch = Scratch::new(name);
        let repo = Repository::init(&scratch.0).expect("repo initialized");
        let mut config = repo.config().expect("config");
        config.set_str("user.name", "Tester").unwrap();
        config
            .set_str("user.email", "tester@example.com")
            .unwrap();
        (scratch, repo)
    }

    #[test]
    fn init_and_detect_uncommitted_changes() {
        let (scratch, _repo) = setup_test_repo("changes");
        scratch.file("a.txt", "hello");
        scratch.file("sub/b.txt", "world");

        let changes = changed_files(&scratch.0).expect("changed files");
        assert_eq!(changes.len(), 2);
        assert!(
            changes
                .iter()
                .any(|c| c.path == Path::new("a.txt") && c.change == Change::Untracked)
        );
        assert!(
            changes
                .iter()
                .any(|c| c.path == Path::new("sub/b.txt") && c.change == Change::Untracked)
        );
    }

    #[test]
    fn commit_stages_only_selected_files() {
        let (scratch, _repo) = setup_test_repo("commit_sel");
        scratch.file("f1.txt", "v1");
        scratch.file("f2.txt", "v1");

        // Commit only f1.txt
        let commit_id =
            commit(&scratch.0, &[PathBuf::from("f1.txt")], "Initial f1").expect("commit ok");
        assert_eq!(commit_id.len(), 40);

        let changes = changed_files(&scratch.0).expect("changed files");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, Path::new("f2.txt"));
        assert_eq!(changes[0].change, Change::Untracked);

        // Now modify f1.txt and commit f2.txt
        scratch.file("f1.txt", "v2");
        commit(&scratch.0, &[PathBuf::from("f2.txt")], "Add f2").expect("commit f2 ok");

        let changes = changed_files(&scratch.0).expect("changed files");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, Path::new("f1.txt"));
        assert_eq!(changes[0].change, Change::Modified);
    }

    #[test]
    fn commit_rejects_empty_message_or_empty_files() {
        let (scratch, _repo) = setup_test_repo("commit_validation");
        scratch.file("f.txt", "content");

        assert!(commit(&scratch.0, &[PathBuf::from("f.txt")], "").is_err());
        assert!(commit(&scratch.0, &[PathBuf::from("f.txt")], "   ").is_err());
        assert!(commit(&scratch.0, &[], "some message").is_err());
    }

    #[test]
    fn unpushed_info_and_push_to_bare_remote() {
        let (scratch, local_repo) = setup_test_repo("local_push");
        let remote_scratch = Scratch::new("remote_bare");
        let _bare = Repository::init_bare(&remote_scratch.0).expect("bare repo init");

        local_repo
            .remote("origin", remote_scratch.0.to_str().unwrap())
            .expect("remote added");

        scratch.file("hello.txt", "1");
        commit(&scratch.0, &[PathBuf::from("hello.txt")], "Commit 1").unwrap();
        scratch.file("hello.txt", "2");
        commit(&scratch.0, &[PathBuf::from("hello.txt")], "Commit 2").unwrap();

        let info = unpushed_info(&scratch.0).expect("unpushed info");
        assert_eq!(info.commits.len(), 2);
        assert_eq!(info.commits[0].message, "Commit 2");
        assert_eq!(info.commits[1].message, "Commit 1");

        // Push to bare remote
        push(&scratch.0, Some("origin"), Some(&info.branch)).expect("push succeeded");

        let after_push = unpushed_info(&scratch.0).expect("unpushed after");
        assert_eq!(after_push.commits.len(), 0);
    }
}
