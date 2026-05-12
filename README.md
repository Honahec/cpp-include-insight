# cpp-include-insight

[![CI](https://github.com/Honahec/cpp-include-insight/actions/workflows/ci.yml/badge.svg)](https://github.com/Honahec/cpp-include-insight/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/cpp-include-insight.svg)](https://crates.io/crates/cpp-include-insight)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

`cpp-include-insight` is an experimental C/C++ include impact analyzer for refactoring, pull request review, and build-time investigation.

It scans direct `#include` directives, builds a project include graph, and answers graph questions such as:

- why one file depends on another
- which files are affected by a header change
- whether project-local includes form cycles
- how to render include graphs for reports or Markdown

## Install

Install the latest published CLI from crates.io:

```bash
cargo install cpp-include-insight --locked
```

Check the installed binary:

```bash
cpp-include-insight --help
```

## Usage

Scan files and print include counts:

```bash
cpp-include-insight scan . -I include
```

Write graph JSON:

```bash
cpp-include-insight graph . -I include --format json
```

Render a Mermaid graph from a root file:

```bash
cpp-include-insight graph src/main.cpp -I include --format mermaid --depth 3 --no-external
```

Inspect include relationships:

```bash
cpp-include-insight tree src/main.cpp -I include
cpp-include-insight rtree include/config.h -I include
cpp-include-insight cycles . -I include
cpp-include-insight why src/main.cpp include/config.h -I include --max-paths 5
cpp-include-insight impact include/config.h -I include
```

## Pull Request Reports

Compare include graph changes between Git revisions and render a Markdown report:

```bash
cpp-include-insight diff main...HEAD -I include
cpp-include-insight report --base main --format markdown -I include
```

`diff A...B` compares the merge-base of `A` and `B` against `B`. `diff A..B`
compares `A` directly against `B`. Git revision diffs read committed content
with `git archive`, so uncommitted worktree changes are not included.

## CI Checks

Run include graph checks locally or in CI:

```bash
cpp-include-insight diff main...HEAD -I include --fail-on-new-cycle
cpp-include-insight ci --base main
```

The `ci` command reads `cpp-include-insight.json`,
`.cpp-include-insight.json`, or `.cpp-include-insight/ci.json` from the Git
root, and can also load an explicit path with `--config`:

```json
{
  "include_dirs": ["include"],
  "ci": {
    "fail_on_new_cycle": true,
    "fail_on_missing_include": true,
    "max_impact_delta": 3,
    "max_new_edges": 10,
    "banned_includes": [
      {
        "from": "include/public/**",
        "to": "src/private/**",
        "reason": "public headers must not include private headers"
      }
    ]
  }
}
```

## GitHub Action

Run include graph checks on pull requests with the bundled GitHub Action. Use
`fetch-depth: 0` so Git revision diffs can find the base ref and merge base.

```yaml
name: Include Insight

on:
  pull_request:

permissions:
  contents: read
  # Required only when `comment: true`, so the action can create or update the
  # stable cpp-include-insight pull request report comment.
  pull-requests: write

jobs:
  include-insight:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0

      - uses: Honahec/cpp-include-insight@v1
        with:
          base: origin/main
          fail-on-new-cycle: true
          comment: true
```

Inputs:

| Input | Default | Description |
| --- | --- | --- |
| `base` | `origin/main` | Base Git revision or ref compared against `HEAD`. |
| `include-dirs` | empty | Whitespace-separated include directories passed as `-I`. |
| `config` | empty | Optional `cpp-include-insight.json` path for CI threshold rules. |
| `fail-on-new-cycle` | `false` | Fail when the PR introduces a new project include cycle. |
| `comment` | `false` | Post or update a pull request comment with the Markdown report. |
| `report-path` | `cpp-include-insight-report.md` | Markdown report output path. |
| `github-token` | `${{ github.token }}` | Token used for PR comments. |

The action always writes a Markdown report and exposes it through the
`report-path` and `report` outputs. When `comment: true`, the action creates a
pull request comment containing `<!-- cpp-include-insight-report -->` and
updates that same marked comment on reruns or force-pushes instead of posting
duplicates. Add `pull-requests: write` only when `comment: true`; otherwise
`contents: read` is enough. Missing or insufficient token permissions fail the
comment step with a message that points back to this permission.

The complete example workflow lives at
`examples/.github/workflows/include-insight.yml`.

## Limitations

The current scanner supports direct includes:

```cpp
#include "app.h"
#include <vector>
# include "foo/bar.hpp"
```

It does not expand macro includes or evaluate conditional compilation. Angle
includes are treated as external in fast scan mode.

## Development

Run the CLI from a checkout:

```bash
cargo run -q -p cpp-include-insight -- scan . -I include
cargo run -q -p cpp-include-insight -- report --base main --format markdown -I include
```

Run the local checks before sending changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --all-features --locked
```

Or use:

```bash
just check
```

## License

MIT. See [LICENSE](./LICENSE).
