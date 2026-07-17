//! Real `git` dependency resolution (§14.2's escape hatch for private/
//! in-development packages, §14.1's `{ git = "...", tag = "..." }` table)
//! — not the full §14.3 story. There's no `packages.ulexite.dev` registry,
//! no lockfile pinning content hashes of transitive dependencies, and no
//! semver-contract checking (§14.4) — those need real server
//! infrastructure and a published-package ecosystem this repo doesn't
//! have. What's real: a `git` dependency's URL is actually cloned via the
//! system `git` binary and checked out at the manifest's declared `tag`,
//! landing in a local, vendored checkout directory under
//! `<project-dir>/.ulexite/git-deps/<hash-of-url-and-tag>/` — the same
//! `path_deps` mechanism an ordinary `{ path = "..." }` dependency already
//! uses, so an import resolves identically either way once the checkout
//! exists. A second resolution against the same (url, tag) reuses that
//! existing checkout rather than re-cloning — real caching, not a fresh
//! clone on every `ulx` invocation.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum GitDepError {
    /// The system has no `git` binary on `PATH` at all.
    GitNotFound,
    Clone {
        url: String,
        stderr: String,
    },
    Checkout {
        tag: String,
        stderr: String,
    },
}

impl std::fmt::Display for GitDepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitDepError::GitNotFound => {
                write!(f, "`git` is not installed (or not on PATH) — required to resolve a git dependency")
            }
            GitDepError::Clone { url, stderr } => {
                write!(f, "failed to clone `{url}`: {}", stderr.trim())
            }
            GitDepError::Checkout { tag, stderr } => {
                write!(f, "failed to check out tag `{tag}`: {}", stderr.trim())
            }
        }
    }
}

/// Where one git dependency's checkout lives, keyed by the exact `(url,
/// tag)` pair so two dependencies naming the same repo at different tags
/// never collide, and changing a tag in `ulexite.toml` resolves into a
/// fresh directory rather than mutating a shared checkout in place (a
/// stale prior checkout under the old key is simply never looked at
/// again, harmless clutter rather than a correctness problem — matching
/// how `.ulexite/cache`'s content-addressed entries are never actively
/// pruned by this v0.1 either).
fn checkout_dir(project_dir: &Path, url: &str, tag: Option<&str>) -> PathBuf {
    let key = format!("{url}@{}", tag.unwrap_or("HEAD"));
    let hash = ulx_runtime::value::hash_bytes(key.as_bytes());
    project_dir
        .join(".ulexite")
        .join("git-deps")
        .join(&hash[..16])
}

fn run_git(args: &[&str]) -> Result<std::process::Output, GitDepError> {
    Command::new("git")
        .args(args)
        .output()
        .map_err(|_| GitDepError::GitNotFound)
}

/// Resolves a git dependency to a local directory: clones (and checks out
/// `tag`, if given) on first use, or returns the existing checkout
/// unchanged on every subsequent call — idempotent, so calling this once
/// per `ulx` invocation (as `pipeline::dependency_paths` does) never
/// re-clones a dependency that's already vendored locally.
pub fn resolve(project_dir: &Path, url: &str, tag: Option<&str>) -> Result<PathBuf, GitDepError> {
    let dir = checkout_dir(project_dir, url, tag);
    if dir.join(".git").exists() {
        return Ok(dir);
    }
    if let Some(parent) = dir.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Remove a possible partial checkout from a prior failed attempt —
    // `git clone` refuses to clone into a nonempty directory.
    let _ = std::fs::remove_dir_all(&dir);

    let clone_output = run_git(&["clone", "--quiet", url, dir.to_str().unwrap_or_default()])?;
    if !clone_output.status.success() {
        return Err(GitDepError::Clone {
            url: url.to_string(),
            stderr: String::from_utf8_lossy(&clone_output.stderr).into_owned(),
        });
    }

    if let Some(tag) = tag {
        let checkout_output = run_git(&[
            "-C",
            dir.to_str().unwrap_or_default(),
            "checkout",
            "--quiet",
            tag,
        ])?;
        if !checkout_output.status.success() {
            return Err(GitDepError::Checkout {
                tag: tag.to_string(),
                stderr: String::from_utf8_lossy(&checkout_output.stderr).into_owned(),
            });
        }
    }

    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a throwaway local git repo with one committed file, tagged
    /// `v1.0.0` — used as a fully offline "git URL" (a local path is a
    /// legitimate clone source for `git`), so these tests never touch the
    /// network.
    fn make_upstream_repo(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .expect("git must be installed to run this test");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "--quiet", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(
            dir.join("marker.ulx"),
            "conversation Marker() -> text { \"v1\" }",
        )
        .unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "initial"]);
        run(&["tag", "v1.0.0"]);
        // A second commit after the tag, so "resolve at v1.0.0" and "resolve
        // at whatever HEAD is now" are genuinely distinguishable outcomes.
        std::fs::write(
            dir.join("marker.ulx"),
            "conversation Marker() -> text { \"v2\" }",
        )
        .unwrap();
        run(&["add", "."]);
        run(&["commit", "--quiet", "-m", "second"]);
    }

    #[test]
    fn resolve_clones_and_checks_out_the_given_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let upstream = tmp.path().join("upstream");
        make_upstream_repo(&upstream);
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let checkout = resolve(&project, upstream.to_str().unwrap(), Some("v1.0.0"))
            .expect("resolve should succeed against a real local repo");
        let content = std::fs::read_to_string(checkout.join("marker.ulx")).unwrap();
        assert!(
            content.contains("\"v1\""),
            "expected the v1.0.0-tagged content, got: {content}"
        );
    }

    #[test]
    fn resolve_without_a_tag_uses_the_default_branch_head() {
        let tmp = tempfile::tempdir().unwrap();
        let upstream = tmp.path().join("upstream");
        make_upstream_repo(&upstream);
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let checkout =
            resolve(&project, upstream.to_str().unwrap(), None).expect("resolve should succeed");
        let content = std::fs::read_to_string(checkout.join("marker.ulx")).unwrap();
        assert!(
            content.contains("\"v2\""),
            "expected HEAD's content (post-tag commit), got: {content}"
        );
    }

    #[test]
    fn a_second_resolve_reuses_the_existing_checkout_rather_than_re_cloning() {
        let tmp = tempfile::tempdir().unwrap();
        let upstream = tmp.path().join("upstream");
        make_upstream_repo(&upstream);
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let first = resolve(&project, upstream.to_str().unwrap(), Some("v1.0.0")).unwrap();
        // Drop a marker file only a fresh clone would wipe out, to prove a
        // second `resolve` call doesn't re-clone over the top of it.
        std::fs::write(first.join("untouched.txt"), "still here").unwrap();

        let second = resolve(&project, upstream.to_str().unwrap(), Some("v1.0.0")).unwrap();
        assert_eq!(
            first, second,
            "same (url, tag) must resolve to the same directory"
        );
        assert!(
            second.join("untouched.txt").exists(),
            "a re-resolve must not re-clone over an existing checkout"
        );
    }

    #[test]
    fn resolve_reports_a_clear_error_for_a_nonexistent_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let bogus = tmp.path().join("does-not-exist");

        let err = resolve(&project, bogus.to_str().unwrap(), None).unwrap_err();
        assert!(matches!(err, GitDepError::Clone { .. }), "{err:?}");
    }

    #[test]
    fn resolve_reports_a_clear_error_for_an_unknown_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let upstream = tmp.path().join("upstream");
        make_upstream_repo(&upstream);
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let err = resolve(&project, upstream.to_str().unwrap(), Some("v99.0.0")).unwrap_err();
        assert!(matches!(err, GitDepError::Checkout { .. }), "{err:?}");
    }
}
