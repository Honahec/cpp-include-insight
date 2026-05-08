use crate::IncludeGraph;
use serde_json::{Value, json};

pub fn graph_to_json_value(graph: &IncludeGraph) -> Value {
    json!({
        "files": graph.files,
        "edges": graph.edges,
        "missing": graph.missing_edges().collect::<Vec<_>>(),
        "external": graph.external_edges().collect::<Vec<_>>(),
    })
}
