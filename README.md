# cpp-include-insight

[![CI](https://github.com/Honahec/cpp-include-insight/actions/workflows/ci.yml/badge.svg)](https://github.com/Honahec/cpp-include-insight/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

`cpp-include-insight` is an experimental C/C++ include impact analyzer for refactoring, pull request review, and build-time investigation.

It scans direct `#include` directives, builds a project include graph, and answers graph questions such as:

- why one file depends on another
- which files are affected by a header change
- whether project-local includes form cycles
- how to render include graphs for reports or Markdown

## Install

Build from source:

```bash
cargo build --workspace
```

Run the CLI:

```bash
cargo run -p cpp-include-insight -- --help
```

## Commands

Scan files and print include counts:

```bash
cargo run -p cpp-include-insight -- scan . -I include
```

Write graph JSON:

```bash
cargo run -p cpp-include-insight -- graph . -I include --format json
```

Render a Mermaid graph from a root file:

```bash
cargo run -p cpp-include-insight -- graph src/main.cpp -I include --format mermaid --depth 3 --no-external
```

Inspect include relationships:

```bash
cargo run -p cpp-include-insight -- tree src/main.cpp -I include
cargo run -p cpp-include-insight -- rtree include/config.h -I include
cargo run -p cpp-include-insight -- cycles . -I include
cargo run -p cpp-include-insight -- why src/main.cpp include/config.h -I include --max-paths 5
cargo run -p cpp-include-insight -- impact include/config.h -I include
```

Compare include graph changes between Git revisions:

```bash
cargo run -p cpp-include-insight -- diff main...HEAD -I include
cargo run -p cpp-include-insight -- diff main...HEAD -I include --fail-on-new-cycle
cargo run -p cpp-include-insight -- ci --base main
```

`diff A...B` compares the merge-base of `A` and `B` against `B`. `diff A..B`
compares `A` directly against `B`. Git revision diffs read committed content
with `git archive`, so uncommitted worktree changes are not included. Use
`--fail-on-new-cycle` to make CI fail when the diff introduces a new include
cycle.

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

## Development

Run the local checks:

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

## Limitations

The current scanner supports direct includes:

```cpp
#include "app.h"
#include <vector>
# include "foo/bar.hpp"
```

It does not expand macro includes or evaluate conditional compilation. Angle includes are treated as external in fast scan mode.

## License

MIT. See [LICENSE](./LICENSE).
