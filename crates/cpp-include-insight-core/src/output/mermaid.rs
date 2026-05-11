use crate::{FileId, IncludeGraph, IncludeTarget};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MermaidOptions {
    pub root: Option<FileId>,
    pub max_depth: Option<usize>,
    pub include_external: bool,
}

pub fn render_mermaid_graph(
    graph: &IncludeGraph,
    base_path: impl AsRef<Path>,
    options: MermaidOptions,
) -> String {
    let base_path = base_path.as_ref();
    let edges = collect_edges(graph, options);
    let mut node_ids = MermaidNodeIds::default();
    let mut output = String::from("graph TD\n");

    if edges.is_empty() {
        if let Some(root) = options.root {
            let node = MermaidNode::Resolved(root);
            output.push_str("  ");
            output.push_str(&node_ids.id_for(&node, graph, base_path));
            output.push_str("[\"");
            output.push_str(&escape_label(&node.label(graph, base_path)));
            output.push_str("\"]\n");
        }

        return output;
    }

    for edge in edges {
        let from = MermaidNode::Resolved(edge.from);
        let to = match &edge.to {
            IncludeTarget::Resolved(id) => MermaidNode::Resolved(*id),
            IncludeTarget::External(path) => MermaidNode::External(path.clone()),
            IncludeTarget::Missing(_) => continue,
        };

        output.push_str("  ");
        output.push_str(&node_ids.id_for(&from, graph, base_path));
        output.push_str("[\"");
        output.push_str(&escape_label(&from.label(graph, base_path)));
        output.push_str("\"] --> ");
        output.push_str(&node_ids.id_for(&to, graph, base_path));
        output.push_str("[\"");
        output.push_str(&escape_label(&to.label(graph, base_path)));
        output.push_str("\"]\n");
    }

    output
}

fn collect_edges(graph: &IncludeGraph, options: MermaidOptions) -> Vec<&crate::IncludeEdge> {
    match options.root {
        Some(root) => collect_reachable_edges(graph, root, options),
        None => graph
            .edges
            .iter()
            .filter(|edge| should_render_edge(edge, options.include_external))
            .collect(),
    }
}

fn collect_reachable_edges(
    graph: &IncludeGraph,
    root: FileId,
    options: MermaidOptions,
) -> Vec<&crate::IncludeEdge> {
    let mut output = Vec::new();
    let mut queue = VecDeque::from([(root, 0usize)]);
    let mut best_depth = HashMap::from([(root, 0usize)]);

    while let Some((file, depth)) = queue.pop_front() {
        if options
            .max_depth
            .is_some_and(|max_depth| depth >= max_depth)
        {
            continue;
        }

        for edge in graph.edges_from(file) {
            if !should_render_edge(edge, options.include_external) {
                continue;
            }

            output.push(edge);

            if let IncludeTarget::Resolved(child) = edge.to {
                let next_depth = depth + 1;
                let should_visit = best_depth
                    .get(&child)
                    .is_none_or(|known_depth| next_depth < *known_depth);

                if should_visit {
                    best_depth.insert(child, next_depth);
                    queue.push_back((child, next_depth));
                }
            }
        }
    }

    output
}

fn should_render_edge(edge: &crate::IncludeEdge, include_external: bool) -> bool {
    match edge.to {
        IncludeTarget::Resolved(_) => true,
        IncludeTarget::External(_) => include_external,
        IncludeTarget::Missing(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MermaidNode {
    Resolved(FileId),
    External(String),
}

impl MermaidNode {
    fn label(&self, graph: &IncludeGraph, base_path: &Path) -> String {
        match self {
            Self::Resolved(id) => display_file(graph, *id, base_path),
            Self::External(path) => format!("<{path}>"),
        }
    }

    fn id_base(&self, graph: &IncludeGraph, base_path: &Path) -> String {
        match self {
            Self::Resolved(_) => stable_node_id(&self.label(graph, base_path)),
            Self::External(path) => stable_node_id(&format!("external_{path}")),
        }
    }

    fn stable_key(&self, graph: &IncludeGraph, base_path: &Path) -> String {
        match self {
            Self::Resolved(id) => format!("file:{}", display_file(graph, *id, base_path)),
            Self::External(path) => format!("external:{path}"),
        }
    }
}

#[derive(Default)]
struct MermaidNodeIds {
    ids_by_node: HashMap<MermaidNode, String>,
    used_ids: HashSet<String>,
}

impl MermaidNodeIds {
    fn id_for(&mut self, node: &MermaidNode, graph: &IncludeGraph, base_path: &Path) -> String {
        if let Some(id) = self.ids_by_node.get(node) {
            return id.clone();
        }

        let base = node.id_base(graph, base_path);
        let id = if self.used_ids.insert(base.clone()) {
            base
        } else {
            let stable_key = node.stable_key(graph, base_path);
            let candidate = format!("{base}_{:08x}", stable_hash(&stable_key));
            self.used_ids.insert(candidate.clone());
            candidate
        };

        self.ids_by_node.insert(node.clone(), id.clone());
        id
    }
}

fn display_file(graph: &IncludeGraph, id: FileId, base_path: &Path) -> String {
    let Some(file) = graph.file(id) else {
        return format!("<unknown:{}>", id.0);
    };

    display_path(&file.path, base_path)
}

fn display_path(path: &Path, base_path: &Path) -> String {
    let display_path = path
        .strip_prefix(base_path)
        .map(PathBuf::from)
        .unwrap_or_else(|_| path.to_path_buf());

    display_path.display().to_string()
}

fn stable_node_id(value: &str) -> String {
    let mut id = String::new();
    let mut previous_was_underscore = false;

    for byte in value.bytes() {
        let next = if byte.is_ascii_alphanumeric() {
            previous_was_underscore = false;
            byte.to_ascii_lowercase() as char
        } else if previous_was_underscore {
            continue;
        } else {
            previous_was_underscore = true;
            '_'
        };

        id.push(next);
    }

    let id = id.trim_matches('_').to_owned();
    let id = if id.is_empty() { "node".to_owned() } else { id };

    if id
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        format!("n_{id}")
    } else {
        id
    }
}

fn stable_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5u32;

    for byte in value.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }

    hash
}

fn escape_label(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileNode, IncludeEdge, IncludeKind};

    #[test]
    fn renders_resolved_and_external_edges() {
        let graph = test_graph();

        assert_eq!(
            render_mermaid_graph(
                &graph,
                "/repo",
                MermaidOptions {
                    root: None,
                    max_depth: None,
                    include_external: true,
                },
            ),
            concat!(
                "graph TD\n",
                "  src_main_cpp[\"src/main.cpp\"] --> include_app_h[\"include/app.h\"]\n",
                "  src_main_cpp[\"src/main.cpp\"] --> external_vector[\"<vector>\"]\n",
                "  include_app_h[\"include/app.h\"] --> include_config_h[\"include/config.h\"]\n",
            )
        );
    }

    #[test]
    fn filters_external_edges() {
        let graph = test_graph();

        assert_eq!(
            render_mermaid_graph(
                &graph,
                "/repo",
                MermaidOptions {
                    root: None,
                    max_depth: None,
                    include_external: false,
                },
            ),
            concat!(
                "graph TD\n",
                "  src_main_cpp[\"src/main.cpp\"] --> include_app_h[\"include/app.h\"]\n",
                "  include_app_h[\"include/app.h\"] --> include_config_h[\"include/config.h\"]\n",
            )
        );
    }

    #[test]
    fn bounds_reachable_edges_by_depth() {
        let graph = test_graph();

        assert_eq!(
            render_mermaid_graph(
                &graph,
                "/repo",
                MermaidOptions {
                    root: Some(FileId(0)),
                    max_depth: Some(1),
                    include_external: true,
                },
            ),
            concat!(
                "graph TD\n",
                "  src_main_cpp[\"src/main.cpp\"] --> include_app_h[\"include/app.h\"]\n",
                "  src_main_cpp[\"src/main.cpp\"] --> external_vector[\"<vector>\"]\n",
            )
        );
    }

    #[test]
    fn renders_root_node_when_depth_excludes_all_edges() {
        let graph = test_graph();

        assert_eq!(
            render_mermaid_graph(
                &graph,
                "/repo",
                MermaidOptions {
                    root: Some(FileId(0)),
                    max_depth: Some(0),
                    include_external: true,
                },
            ),
            "graph TD\n  src_main_cpp[\"src/main.cpp\"]\n"
        );
    }

    fn test_graph() -> IncludeGraph {
        IncludeGraph {
            files: vec![
                FileNode {
                    id: FileId(0),
                    path: PathBuf::from("/repo/src/main.cpp"),
                },
                FileNode {
                    id: FileId(1),
                    path: PathBuf::from("/repo/include/app.h"),
                },
                FileNode {
                    id: FileId(2),
                    path: PathBuf::from("/repo/include/config.h"),
                },
            ],
            edges: vec![
                test_edge(0, IncludeTarget::Resolved(FileId(1)), "app.h"),
                test_edge(0, IncludeTarget::External("vector".to_owned()), "vector"),
                test_edge(1, IncludeTarget::Resolved(FileId(2)), "config.h"),
                test_edge(
                    2,
                    IncludeTarget::Missing("missing.h".to_owned()),
                    "missing.h",
                ),
            ],
        }
    }

    fn test_edge(from: usize, to: IncludeTarget, include_path: &str) -> IncludeEdge {
        IncludeEdge {
            from: FileId(from),
            to,
            include_path: include_path.to_owned(),
            kind: IncludeKind::Quote,
            line: 1,
        }
    }
}
