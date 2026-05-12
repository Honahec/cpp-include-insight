#!/usr/bin/env bash
set -euo pipefail

readonly MARKER="<!-- cpp-include-insight-report -->"

usage() {
  echo "usage: comment-pr-report.sh <report-path> <pr-number> <owner/repo>" >&2
}

fail() {
  echo "cpp-include-insight: $*" >&2
  exit 1
}

fail_gh() {
  local action="$1"
  local stderr_file="$2"

  echo "cpp-include-insight: failed to ${action}." >&2
  if [[ -s "$stderr_file" ]]; then
    cat "$stderr_file" >&2
  fi
  echo "cpp-include-insight: ensure the workflow token can write pull request comments." >&2
  echo "cpp-include-insight: add 'pull-requests: write' to workflow permissions when comment: true." >&2
  exit 1
}

if [[ $# -ne 3 ]]; then
  usage
  exit 2
fi

report_path="$1"
pr_number="$2"
repository="$3"

[[ -f "$report_path" ]] || fail "report file does not exist: $report_path"
[[ -n "${GH_TOKEN:-}" ]] || fail "GH_TOKEN is required to publish pull request comments."
command -v gh >/dev/null 2>&1 || fail "GitHub CLI 'gh' is required to publish pull request comments."

temp_dir=""
if [[ -n "${RUNNER_TEMP:-}" ]]; then
  work_dir="$RUNNER_TEMP"
else
  temp_dir="$(mktemp -d)"
  work_dir="$temp_dir"
  trap 'rm -rf "$temp_dir"' EXIT
fi

body_file="$work_dir/cpp-include-insight-comment.md"
stderr_file="$work_dir/cpp-include-insight-gh-error.log"

{
  printf '%s\n\n' "$MARKER"
  cat "$report_path"
} > "$body_file"

if ! comment_id="$(
  gh api "repos/$repository/issues/$pr_number/comments" \
    --jq ".[] | select(.body | contains(\"$MARKER\")) | .id" \
    2> "$stderr_file" \
    | awk 'NF { print; exit }'
)"; then
  fail_gh "list pull request comments" "$stderr_file"
fi

if [[ -n "$comment_id" ]]; then
  if ! gh api \
    --method PATCH \
    "repos/$repository/issues/comments/$comment_id" \
    --field body=@"$body_file" \
    >/dev/null 2> "$stderr_file"; then
    fail_gh "update existing cpp-include-insight pull request comment" "$stderr_file"
  fi
else
  if ! gh pr comment "$pr_number" \
    --repo "$repository" \
    --body-file "$body_file" \
    >/dev/null 2> "$stderr_file"; then
    fail_gh "create cpp-include-insight pull request comment" "$stderr_file"
  fi
fi
