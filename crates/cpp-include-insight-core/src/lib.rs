pub mod analysis;
pub mod graph;
pub mod output;
pub mod parser;
pub mod resolver;
pub mod scanner;

pub use analysis::cycles::{IncludeCycle, detect_include_cycles, render_include_cycles};
pub use analysis::impact::{
    FileKind, ImpactResult, analyze_include_impact, classify_file, render_impact_result,
};
pub use analysis::why::{
    DEFAULT_MAX_WHY_PATHS, IncludePath, WhyOptions, WhyResult, find_include_paths,
    render_why_result,
};
pub use graph::{FileId, FileNode, IncludeEdge, IncludeGraph, IncludeGraphStats, IncludeTarget};
pub use output::diff::{
    MissingIncludeChange, ResolvedDependencyChange, SnapshotCycleChange, SnapshotDiff,
    diff_include_graph_snapshots, load_include_graph_snapshot, render_snapshot_diff,
    validate_include_graph_snapshot,
};
pub use output::json::graph_to_json_value;
pub use output::mermaid::{MermaidOptions, render_mermaid_graph};
pub use output::snapshot::{
    IncludeGraphSnapshot, SNAPSHOT_VERSION, SnapshotOptions, build_include_graph_snapshot,
};
pub use output::tree::{render_include_tree, render_reverse_include_tree};
pub use parser::{IncludeDirective, IncludeKind};
pub use resolver::{IncludeResolution, IncludeResolver};
pub use scanner::{ScanOptions, ScanResult, scan_project};
