use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn run_cli(args: &[&str]) -> String {
    run_cli_in(args, None)
}

fn run_cli_in(args: &[&str], current_dir: Option<&Path>) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cpp-include-insight"));
    command.args(args);

    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }

    let output = command.output().expect("failed to run cpp-include-insight");

    assert!(
        output.status.success(),
        "command failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8(output.stdout).expect("stdout should be valid UTF-8")
}

#[test]
fn graph_json_outputs_files_edges_missing_and_external_for_simple_fixture() {
    let fixture = fixture_path("simple");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["graph", fixture, "-I", "include", "--format", "json"]);
    let json: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["files"].as_array().unwrap().len(), 3);
    assert_eq!(json["edges"].as_array().unwrap().len(), 5);
    assert_eq!(json["missing"].as_array().unwrap().len(), 0);
    assert_eq!(json["external"].as_array().unwrap().len(), 3);

    let edges = json["edges"].as_array().unwrap();
    assert!(edges.iter().any(|edge| edge["include_path"] == "app.h"
        && edge["kind"] == "quote"
        && edge["to"].get("resolved").is_some()));
    assert!(edges.iter().any(|edge| edge["include_path"] == "vector"
        && edge["kind"] == "angle"
        && edge["to"] == serde_json::json!({ "external": "vector" })));
}

#[test]
fn graph_json_reports_missing_includes() {
    let fixture = fixture_path("missing");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["graph", fixture, "--format", "json"]);
    let json: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["files"].as_array().unwrap().len(), 1);
    assert_eq!(json["edges"].as_array().unwrap().len(), 1);
    assert_eq!(json["external"].as_array().unwrap().len(), 0);

    let missing = json["missing"].as_array().unwrap();
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0]["include_path"], "missing.h");
    assert_eq!(
        missing[0]["to"],
        serde_json::json!({ "missing": "missing.h" })
    );
}

#[test]
fn graph_json_reports_external_includes() {
    let fixture = fixture_path("external-includes");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["graph", fixture, "--format", "json"]);
    let json: Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["files"].as_array().unwrap().len(), 1);
    assert_eq!(json["edges"].as_array().unwrap().len(), 1);
    assert_eq!(json["missing"].as_array().unwrap().len(), 0);

    let external = json["external"].as_array().unwrap();
    assert_eq!(external.len(), 1);
    assert_eq!(external[0]["include_path"], "vector");
    assert_eq!(
        external[0]["to"],
        serde_json::json!({ "external": "vector" })
    );
}

#[test]
fn graph_mermaid_outputs_basic_include_graph_from_root_file() {
    let fixture = fixture_path("tree-normal");
    let stdout = run_cli_in(
        &[
            "graph",
            "src/main.cpp",
            "-I",
            "include",
            "--format",
            "mermaid",
        ],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "graph TD\n",
            "  src_main_cpp[\"src/main.cpp\"] --> include_app_h[\"include/app.h\"]\n",
            "  include_app_h[\"include/app.h\"] --> include_config_h[\"include/config.h\"]\n",
        )
    );
}

#[test]
fn graph_mermaid_respects_depth_from_root_file() {
    let fixture = fixture_path("tree-normal");
    let stdout = run_cli_in(
        &[
            "graph",
            "src/main.cpp",
            "-I",
            "include",
            "--format",
            "mermaid",
            "--depth",
            "1",
        ],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "graph TD\n",
            "  src_main_cpp[\"src/main.cpp\"] --> include_app_h[\"include/app.h\"]\n",
        )
    );
}

#[test]
fn graph_mermaid_can_omit_external_includes() {
    let fixture = fixture_path("simple");
    let stdout = run_cli_in(
        &[
            "graph",
            "src/main.cpp",
            "-I",
            "include",
            "--format",
            "mermaid",
            "--no-external",
        ],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "graph TD\n",
            "  src_main_cpp[\"src/main.cpp\"] --> include_app_h[\"include/app.h\"]\n",
            "  src_main_cpp[\"src/main.cpp\"] --> src_local_h[\"src/local.h\"]\n",
        )
    );
}

#[test]
fn graph_mermaid_includes_external_includes_by_default() {
    let fixture = fixture_path("simple");
    let stdout = run_cli_in(
        &[
            "graph",
            "src/main.cpp",
            "-I",
            "include",
            "--format",
            "mermaid",
        ],
        Some(&fixture),
    );

    assert!(stdout.contains("external_vector[\"<vector>\"]"));
    assert!(stdout.contains("external_string[\"<string>\"]"));
    assert!(stdout.contains("external_math_h[\"<math.h>\"]"));
}

#[test]
fn scan_text_reports_resolution_counts() {
    let fixture = fixture_path("simple");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["scan", fixture, "-I", "include"]);

    assert!(stdout.contains("Scanned 3 files."));
    assert!(stdout.contains("Found 5 include directives."));
    assert!(stdout.contains("Resolved 2 project includes."));
    assert!(stdout.contains("External includes: 3"));
    assert!(stdout.contains("Missing includes: 0"));
}

#[test]
fn tree_outputs_forward_include_tree() {
    let fixture = fixture_path("tree-normal");
    let stdout = run_cli_in(&["tree", "src/main.cpp", "-I", "include"], Some(&fixture));

    assert_eq!(
        stdout,
        "src/main.cpp\n`-- include/app.h\n    `-- include/config.h\n"
    );
}

#[test]
fn tree_marks_repeated_nodes() {
    let fixture = fixture_path("tree-repeated");
    let stdout = run_cli_in(&["tree", "src/main.cpp", "-I", "include"], Some(&fixture));

    assert_eq!(
        stdout,
        concat!(
            "src/main.cpp\n",
            "|-- include/a.h\n",
            "|   `-- include/shared.h\n",
            "`-- include/b.h\n",
            "    `-- include/shared.h [already shown]\n",
        )
    );
}

#[test]
fn tree_marks_cycles_without_recursing_forever() {
    let fixture = fixture_path("tree-cycle");
    let stdout = run_cli_in(&["tree", "src/main.cpp", "-I", "include"], Some(&fixture));

    assert_eq!(
        stdout,
        concat!(
            "src/main.cpp\n",
            "`-- include/a.h\n",
            "    `-- include/b.h\n",
            "        `-- include/a.h [cycle]\n",
        )
    );
}

#[test]
fn rtree_outputs_direct_dependants() {
    let fixture = fixture_path("rtree-direct");
    let stdout = run_cli_in(
        &["rtree", "include/config.h", "-I", "include"],
        Some(&fixture),
    );

    assert_eq!(stdout, "include/config.h\n`-- src/main.cpp\n");
}

#[test]
fn rtree_outputs_transitive_dependants() {
    let fixture = fixture_path("tree-normal");
    let stdout = run_cli_in(
        &["rtree", "include/config.h", "-I", "include"],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        "include/config.h\n`-- include/app.h\n    `-- src/main.cpp\n"
    );
}

#[test]
fn rtree_marks_repeated_nodes() {
    let fixture = fixture_path("tree-repeated");
    let stdout = run_cli_in(
        &["rtree", "include/shared.h", "-I", "include"],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "include/shared.h\n",
            "|-- include/a.h\n",
            "|   `-- src/main.cpp\n",
            "`-- include/b.h\n",
            "    `-- src/main.cpp [already shown]\n",
        )
    );
}

#[test]
fn rtree_marks_cycles_without_recursing_forever() {
    let fixture = fixture_path("tree-cycle");
    let stdout = run_cli_in(&["rtree", "include/a.h", "-I", "include"], Some(&fixture));

    assert_eq!(
        stdout,
        concat!(
            "include/a.h\n",
            "|-- include/b.h\n",
            "|   `-- include/a.h [cycle]\n",
            "`-- src/main.cpp\n",
        )
    );
}

#[test]
fn cycles_reports_no_cycles() {
    let fixture = fixture_path("tree-normal");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["cycles", fixture, "-I", "include"]);

    assert_eq!(stdout, "Found 0 include cycles.\n");
}

#[test]
fn cycles_reports_single_cycle_with_line_metadata() {
    let fixture = fixture_path("tree-cycle");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["cycles", fixture, "-I", "include"]);

    assert_eq!(
        stdout,
        concat!(
            "Found 1 include cycle.\n",
            "\n",
            "Cycle 1:\n",
            "include/a.h:3\n",
            "  -> include/b.h:3\n",
            "  -> include/a.h\n",
        )
    );
}

#[test]
fn cycles_reports_multiple_cycles_and_ignores_unresolved_targets() {
    let fixture = fixture_path("cycles-multiple");
    let fixture = fixture.to_str().unwrap();
    let stdout = run_cli(&["cycles", fixture, "-I", "include"]);

    assert_eq!(
        stdout,
        concat!(
            "Found 2 include cycles.\n",
            "\n",
            "Cycle 1:\n",
            "include/a.h:3\n",
            "  -> include/b.h:3\n",
            "  -> include/a.h\n",
            "\n",
            "Cycle 2:\n",
            "include/c.h:3\n",
            "  -> include/d.h:3\n",
            "  -> include/c.h\n",
        )
    );
}

#[test]
fn why_reports_one_dependency_path() {
    let fixture = fixture_path("why-one-path");
    let stdout = run_cli_in(
        &["why", "src/main.cpp", "include/config.h", "-I", "include"],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "src/main.cpp depends on include/config.h through 1 path.\n",
            "\n",
            "Path 1:\n",
            "src/main.cpp:1\n",
            "  -> include/app.h:3\n",
            "  -> include/config.h\n",
        )
    );
}

#[test]
fn why_reports_multiple_dependency_paths() {
    let fixture = fixture_path("why-multiple-paths");
    let stdout = run_cli_in(
        &[
            "why",
            "src/main.cpp",
            "include/config.h",
            "-I",
            "include",
            "--all",
        ],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "src/main.cpp depends on include/config.h through 3 paths.\n",
            "\n",
            "Path 1:\n",
            "src/main.cpp:1\n",
            "  -> include/app.h:3\n",
            "  -> include/config.h\n",
            "\n",
            "Path 2:\n",
            "src/main.cpp:2\n",
            "  -> include/logger.h:3\n",
            "  -> include/config.h\n",
            "\n",
            "Path 3:\n",
            "src/main.cpp:3\n",
            "  -> include/runtime.h:3\n",
            "  -> include/settings.h:3\n",
            "  -> include/config.h\n",
        )
    );
}

#[test]
fn why_reports_no_dependency_path() {
    let fixture = fixture_path("why-no-path");
    let stdout = run_cli_in(
        &["why", "src/main.cpp", "include/config.h", "-I", "include"],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        "src/main.cpp does not depend on include/config.h.\n"
    );
}

#[test]
fn why_respects_max_paths_bound() {
    let fixture = fixture_path("why-multiple-paths");
    let stdout = run_cli_in(
        &[
            "why",
            "src/main.cpp",
            "include/config.h",
            "-I",
            "include",
            "--max-paths",
            "2",
        ],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "src/main.cpp depends on include/config.h through 2 paths.\n",
            "Output was bounded; use --all to search exhaustively.\n",
            "\n",
            "Path 1:\n",
            "src/main.cpp:1\n",
            "  -> include/app.h:3\n",
            "  -> include/config.h\n",
            "\n",
            "Path 2:\n",
            "src/main.cpp:2\n",
            "  -> include/logger.h:3\n",
            "  -> include/config.h\n",
        )
    );
}

#[test]
fn why_shortest_reports_only_the_shortest_path() {
    let fixture = fixture_path("why-multiple-paths");
    let stdout = run_cli_in(
        &[
            "why",
            "src/main.cpp",
            "include/config.h",
            "-I",
            "include",
            "--shortest",
        ],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "src/main.cpp depends on include/config.h through 1 path.\n",
            "\n",
            "Path 1:\n",
            "src/main.cpp:1\n",
            "  -> include/app.h:3\n",
            "  -> include/config.h\n",
        )
    );
}

#[test]
fn impact_reports_direct_transitive_and_translation_unit_dependants() {
    let fixture = fixture_path("impact");
    let stdout = run_cli_in(
        &["impact", "include/config.h", "-I", "include"],
        Some(&fixture),
    );

    assert_eq!(
        stdout,
        concat!(
            "include/config.h impacts 6 files.\n",
            "\n",
            "Translation units:\n",
            "  src/legacy.c\n",
            "  src/main.cpp\n",
            "  src/server.cxx\n",
            "  src/worker.cc\n",
            "\n",
            "Headers:\n",
            "  include/app.h\n",
            "  include/logger.hpp\n",
            "\n",
            "Direct dependants:\n",
            "  include/app.h\n",
            "  include/logger.hpp\n",
            "  src/worker.cc\n",
            "\n",
            "Transitive dependants:\n",
            "  src/legacy.c\n",
            "  src/main.cpp\n",
            "  src/server.cxx\n",
        )
    );
}

#[test]
fn snapshot_writes_project_relative_versioned_json() {
    let fixture = fixture_path("simple");
    let output_dir = tempfile::tempdir().unwrap();
    let output_file = output_dir.path().join("include-graph.json");
    let output_file_arg = output_file.to_str().unwrap();
    let fixture_arg = fixture.to_str().unwrap();

    let stdout = run_cli(&[
        "snapshot",
        fixture_arg,
        "-I",
        "include",
        "-o",
        output_file_arg,
    ]);

    assert_eq!(stdout, "");

    let json: Value = serde_json::from_str(&fs::read_to_string(&output_file).unwrap()).unwrap();

    assert_eq!(json["version"], 1);
    assert_eq!(
        json["root"],
        serde_json::json!({
            "path": ".",
            "path_style": "project_relative",
        })
    );
    assert_eq!(
        json["stats"],
        serde_json::json!({
            "files": 3,
            "edges": 5,
            "resolved": 2,
            "external": 3,
            "missing": 0,
            "cycles": 0,
        })
    );
    assert_eq!(
        json["files"],
        serde_json::json!([
            { "path": "include/app.h" },
            { "path": "src/local.h" },
            { "path": "src/main.cpp" },
        ])
    );

    let edges = json["edges"].as_array().unwrap();
    assert_eq!(edges[0]["from"], "include/app.h");
    assert_eq!(
        edges[0]["to"],
        serde_json::json!({ "type": "external", "include": "string" })
    );
    assert_eq!(edges[1]["include"], "math.h");
    assert_eq!(edges[2]["include"], "app.h");
    assert_eq!(
        edges[2]["to"],
        serde_json::json!({ "type": "resolved", "path": "include/app.h" })
    );
    assert_eq!(json["cycles"], serde_json::json!([]));

    let serialized = fs::read_to_string(&output_file).unwrap();
    assert!(!serialized.contains(fixture_arg));
}

#[test]
fn snapshot_output_is_deterministic_and_includes_cycles() {
    let fixture = fixture_path("tree-cycle");
    let output_dir = tempfile::tempdir().unwrap();
    let first_output = output_dir.path().join("first.json");
    let second_output = output_dir.path().join("second.json");
    let fixture_arg = fixture.to_str().unwrap();

    run_cli(&[
        "snapshot",
        fixture_arg,
        "-I",
        "include",
        "-o",
        first_output.to_str().unwrap(),
    ]);
    run_cli(&[
        "snapshot",
        fixture_arg,
        "-I",
        "include",
        "-o",
        second_output.to_str().unwrap(),
    ]);

    let first = fs::read_to_string(first_output).unwrap();
    let second = fs::read_to_string(second_output).unwrap();
    assert_eq!(first, second);

    let json: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(json["stats"]["cycles"], 1);
    assert_eq!(
        json["cycles"][0]["files"],
        serde_json::json!(["include/a.h", "include/b.h"])
    );
    assert_eq!(
        json["cycles"][0]["edges"],
        serde_json::json!([
            {
                "from": "include/a.h",
                "to": "include/b.h",
                "include": "b.h",
                "line": 3,
            },
            {
                "from": "include/b.h",
                "to": "include/a.h",
                "include": "a.h",
                "line": 3,
            },
        ])
    );
}

#[test]
fn diff_reports_snapshot_dependency_changes_in_stable_order() {
    let fixture = fixture_path("snapshot-diff");
    let old = fixture.join("old.json");
    let new = fixture.join("new.json");
    let stdout = run_cli(&["diff", old.to_str().unwrap(), new.to_str().unwrap()]);

    assert_eq!(
        stdout,
        concat!(
            "Snapshot diff summary:\n",
            "Files: 3 -> 4 (+1)\n",
            "Edges: 4 -> 5 (+1)\n",
            "Resolved: 2 -> 3 (+1)\n",
            "External: 1 -> 1 (+0)\n",
            "Missing: 1 -> 1 (+0)\n",
            "Cycles: 0 -> 0 (+0)\n",
            "\n",
            "Added resolved dependencies (2):\n",
            "  src/main.cpp:3 -> include/generated.h (include \"generated.h\")\n",
            "  src/main.cpp:4 -> include/new.h (include \"new.h\")\n",
            "\n",
            "Removed resolved dependencies (1):\n",
            "  src/main.cpp:2 -> include/old.h (include \"old.h\")\n",
            "\n",
            "Newly missing includes (1):\n",
            "  include/app.h:2 -> missing config.h (include \"config.h\")\n",
            "\n",
            "Newly resolved includes (1):\n",
            "  src/main.cpp:3 -> include/generated.h (include \"generated.h\")\n",
        )
    );
}
