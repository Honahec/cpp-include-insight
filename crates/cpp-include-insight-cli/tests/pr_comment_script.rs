use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const MARKER: &str = "<!-- cpp-include-insight-report -->";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn script_path() -> PathBuf {
    repo_root().join("scripts/comment-pr-report.sh")
}

struct FakeGh {
    temp: tempfile::TempDir,
    report_path: PathBuf,
    body_copy_path: PathBuf,
    log_path: PathBuf,
}

impl FakeGh {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let report_path = temp.path().join("report.md");
        let body_copy_path = temp.path().join("body.md");
        let log_path = temp.path().join("gh.log");
        let bin_dir = temp.path().join("bin");

        fs::create_dir(&bin_dir).unwrap();
        fs::write(
            &report_path,
            "## cpp-include-insight Report\n\nNo include graph changes.\n",
        )
        .unwrap();
        fs::write(bin_dir.join("gh"), fake_gh_script()).unwrap();

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(bin_dir.join("gh")).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(bin_dir.join("gh"), permissions).unwrap();
        }

        Self {
            temp,
            report_path,
            body_copy_path,
            log_path,
        }
    }

    fn run(&self, mode: &str) -> Output {
        let path = format!(
            "{}:{}",
            self.temp.path().join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );

        Command::new("bash")
            .arg(script_path())
            .arg(&self.report_path)
            .arg("40")
            .arg("owner/repo")
            .env("FAKE_GH_MODE", mode)
            .env("FAKE_GH_LOG", &self.log_path)
            .env("FAKE_BODY_COPY", &self.body_copy_path)
            .env("GH_TOKEN", "test-token")
            .env("RUNNER_TEMP", self.temp.path())
            .env("PATH", path)
            .output()
            .expect("failed to run comment-pr-report.sh")
    }

    fn log(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap()
    }

    fn copied_body(&self) -> String {
        fs::read_to_string(&self.body_copy_path).unwrap()
    }
}

#[test]
fn pr_comment_script_creates_marked_comment_when_none_exists() {
    let fake = FakeGh::new();
    let output = fake.run("create");

    assert_success(&output);

    let log = fake.log();
    assert!(log.contains("api repos/owner/repo/issues/40/comments --jq"));
    assert!(log.contains(MARKER));
    assert!(log.contains("pr comment 40 --repo owner/repo --body-file"));
    assert_eq!(
        fake.copied_body(),
        concat!(
            "<!-- cpp-include-insight-report -->\n",
            "\n",
            "## cpp-include-insight Report\n",
            "\n",
            "No include graph changes.\n",
        )
    );
}

#[test]
fn pr_comment_script_updates_marked_comment_when_it_exists() {
    let fake = FakeGh::new();
    let output = fake.run("update");

    assert_success(&output);

    let log = fake.log();
    assert!(log.contains("api repos/owner/repo/issues/40/comments --jq"));
    assert!(log.contains("api --method PATCH repos/owner/repo/issues/comments/123456 --field"));
    assert!(!log.contains("pr comment"));
    assert!(fake.copied_body().starts_with(MARKER));
}

#[test]
fn pr_comment_script_reports_clear_permission_errors() {
    let fake = FakeGh::new();
    let output = fake.run("permission");

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("failed to list pull request comments"));
    assert!(stderr.contains("HTTP 403: Resource not accessible by integration"));
    assert!(stderr.contains("pull-requests: write"));
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fake_gh_script() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail

echo "$*" >> "$FAKE_GH_LOG"

copy_body_file() {
  local body_file=""
  local previous=""

  for argument in "$@"; do
    if [[ "$previous" == "--body-file" ]]; then
      body_file="$argument"
    elif [[ "$argument" == body=@* ]]; then
      body_file="${argument#body=@}"
    fi
    previous="$argument"
  done

  if [[ -n "$body_file" ]]; then
    cp "$body_file" "$FAKE_BODY_COPY"
  fi
}

mode="${FAKE_GH_MODE:-create}"

if [[ "$mode" == "permission" && "${1:-}" == "api" && "${2:-}" == repos/*/issues/*/comments ]]; then
  echo "HTTP 403: Resource not accessible by integration" >&2
  exit 1
fi

if [[ "${1:-}" == "api" && "${2:-}" == repos/*/issues/*/comments ]]; then
  if [[ "$mode" == "update" ]]; then
    echo "123456"
  fi
  exit 0
fi

if [[ "${1:-}" == "api" ]]; then
  copy_body_file "$@"
  exit 0
fi

if [[ "${1:-}" == "pr" && "${2:-}" == "comment" ]]; then
  copy_body_file "$@"
  exit 0
fi

echo "unexpected gh invocation: $*" >&2
exit 9
"#
}
