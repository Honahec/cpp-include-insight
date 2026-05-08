# cpp-include-insight

[![CI](https://github.com/Honahec/cpp-include-insight/actions/workflows/ci.yml/badge.svg)](https://github.com/Honahec/cpp-include-insight/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

`cpp-include-insight` is a C/C++ include impact analyzer for pull requests, refactoring, and build-time optimization.

The goal of this project is not to be another simple include graph generator. Instead, it aims to help developers understand how header dependencies affect a C/C++ project over time.

## Status

This project is currently experimental and under active development.

The first milestone focuses on:

- scanning C/C++ source files
- extracting `#include` directives
- resolving project-local includes
- building an include dependency graph
- emitting graph JSON for downstream analysis

## Motivation

C/C++ header dependencies can easily become hard to understand in large projects.

Common questions include:

- Why is this header included?
- Which translation units are affected if this header changes?
- Did this pull request introduce new include dependencies?
- Did this pull request introduce a new include cycle?
- Which headers have the largest transitive impact?
- Which includes may be increasing build time?

`cpp-include-insight` is designed to answer these questions using include graph analysis.

## Goals

- Scan C/C++ files and extract direct `#include` directives
- Build a project include dependency graph
- Explain why one file depends on another
- Estimate the impact of changing a header
- Detect include cycles
- Compare include graphs between two revisions
- Generate Markdown reports for pull requests
- Support CI checks for include dependency regressions

## Non-goals

This project does not aim to replace existing C/C++ tooling such as:

- `clangd`
- `clang-tidy`
- Include What You Use
- Clang dependency scanning tools

In particular, `cpp-include-insight` is not intended to be:

- a full C++ parser
- a full C/C++ preprocessor
- a semantic unused-include analyzer
- a replacement for compiler diagnostics

The focus is include graph insight, especially for refactoring and pull request review.

## Installation

At the moment, the project is intended to be built from source.

```bash
cargo build --workspace
```

Run the CLI:

```bash
cargo run -p cpp-include-insight -- --help
```

## Quick Start

Scan a project:

```bash
cargo run -p cpp-include-insight -- scan . -I include
```

Example output:

```text
Scanned 3 files.
Found 5 include directives.
Resolved 2 project includes.
External includes: 3
Missing includes: 0
tests/fixtures/simple/include/app.h:3 -> string
tests/fixtures/simple/src/local.h:3 -> math.h
tests/fixtures/simple/src/main.cpp:1 -> app.h
tests/fixtures/simple/src/main.cpp:2 -> local.h
tests/fixtures/simple/src/main.cpp:3 -> vector
```

Build the include graph and output JSON:

```bash
cargo run -p cpp-include-insight -- graph . -I include --format json
```

Example output:

```json
{
  "files": [
    {
      "id": 0,
      "path": "tests/fixtures/simple/include/app.h"
    }
  ],
  "edges": [
    {
      "from": 2,
      "to": {
        "resolved": 0
      },
      "include_path": "app.h",
      "kind": "quote",
      "line": 1
    }
  ],
  "missing": [],
  "external": []
}
```

## Planned CLI

The final CLI is expected to provide commands such as:

```bash
cpp-include-insight scan .
cpp-include-insight graph . --format json
cpp-include-insight graph src/main.cpp --format mermaid
cpp-include-insight tree src/main.cpp
cpp-include-insight rtree include/config.h
cpp-include-insight cycles .
cpp-include-insight why src/main.cpp include/config.h
cpp-include-insight impact include/config.h
cpp-include-insight diff main...HEAD
cpp-include-insight report --base main --format markdown
cpp-include-insight ci --base main
```

These commands will be implemented incrementally.

## Project Layout

```text
cpp-include-insight/
├── crates/
│   ├── cpp-include-insight-core/
│   │   └── src/
│   │       ├── parser.rs
│   │       ├── scanner.rs
│   │       ├── resolver.rs
│   │       ├── graph.rs
│   │       ├── analysis/
│   │       └── output/
│   └── cpp-include-insight-cli/
│       └── src/
│           └── main.rs
├── tests/
│   └── fixtures/
├── .github/
│   └── workflows/
│       └── ci.yml
├── Cargo.toml
├── README.md
└── LICENSE
```

## Development

Run formatting:

```bash
cargo fmt --all
```

Run Clippy:

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Run tests:

```bash
cargo test --workspace --all-features
```

Build the workspace:

```bash
cargo build --workspace --all-features --locked
```

Run all local checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --all-features --locked
```

## Limitations

The initial implementation only supports direct include directives such as:

```cpp
#include "app.h"
#include <vector>
# include "foo/bar.hpp"
```

The following cases are not fully supported yet:

```cpp
#define HEADER "app.h"
#include HEADER
```

```cpp
#ifdef USE_SSL
#include "ssl_backend.h"
#else
#include "plain_backend.h"
#endif
```

In fast scan mode, conditional includes may be recorded conservatively.

## Contributing

Contributions are welcome.

Before opening a pull request, please run:

```bash
just check
```

## License

This project is licensed under the MIT License.

See [LICENSE](./LICENSE) for details.
