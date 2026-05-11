use crate::{FileId, IncludeEdge, IncludeGraph, IncludeTarget};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};

pub const DEFAULT_MAX_WHY_PATHS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyOptions {
    pub max_paths: Option<usize>,
    pub shortest: bool,
}

impl Default for WhyOptions {
    fn default() -> Self {
        Self {
            max_paths: Some(DEFAULT_MAX_WHY_PATHS),
            shortest: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludePath {
    pub edges: Vec<IncludeEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyResult {
    pub source: FileId,
    pub target: FileId,
    pub paths: Vec<IncludePath>,
    pub truncated: bool,
}

pub fn find_include_paths(
    graph: &IncludeGraph,
    source: FileId,
    target: FileId,
    options: &WhyOptions,
) -> WhyResult {
    if source == target {
        return WhyResult {
            source,
            target,
            paths: vec![IncludePath { edges: Vec::new() }],
            truncated: false,
        };
    }

    if options.shortest {
        return find_shortest_include_path(graph, source, target);
    }

    let search_limit = options
        .max_paths
        .map(|max_paths| max_paths.saturating_add(1));
    let mut finder = PathFinder::new(graph, target, search_limit);
    finder.search(source);

    let mut paths = finder.paths;
    let mut truncated = false;

    if let Some(max_paths) = options.max_paths
        && paths.len() > max_paths
    {
        paths.truncate(max_paths);
        truncated = true;
    }

    WhyResult {
        source,
        target,
        paths,
        truncated,
    }
}

fn find_shortest_include_path(graph: &IncludeGraph, source: FileId, target: FileId) -> WhyResult {
    let mut seen = HashSet::from([source]);
    let mut queue = VecDeque::from([PathCandidate {
        file: source,
        edges: Vec::new(),
    }]);

    while let Some(candidate) = queue.pop_front() {
        for edge in sorted_resolved_edges_from(graph, candidate.file) {
            let IncludeTarget::Resolved(next) = edge.to else {
                continue;
            };

            if !seen.insert(next) {
                continue;
            }

            let mut edges = candidate.edges.clone();
            edges.push(edge);

            if next == target {
                return WhyResult {
                    source,
                    target,
                    paths: vec![IncludePath { edges }],
                    truncated: false,
                };
            }

            queue.push_back(PathCandidate { file: next, edges });
        }
    }

    WhyResult {
        source,
        target,
        paths: Vec::new(),
        truncated: false,
    }
}

#[derive(Debug, Clone)]
struct PathCandidate {
    file: FileId,
    edges: Vec<IncludeEdge>,
}

pub fn render_why_result(
    graph: &IncludeGraph,
    result: &WhyResult,
    base_path: impl AsRef<Path>,
) -> String {
    let base_path = base_path.as_ref();
    let source = display_file(graph, result.source, base_path);
    let target = display_file(graph, result.target, base_path);

    if result.paths.is_empty() {
        return format!("{source} does not depend on {target}.\n");
    }

    let suffix = if result.paths.len() == 1 {
        "path"
    } else {
        "paths"
    };
    let mut output = format!(
        "{source} depends on {target} through {} {suffix}.",
        result.paths.len()
    );
    output.push('\n');

    if result.truncated {
        output.push_str("Output was bounded; use --all to search exhaustively.\n");
    }

    output.push('\n');

    for (index, path) in result.paths.iter().enumerate() {
        output.push_str(&format!("Path {}:\n", index + 1));
        render_path(graph, path, base_path, &mut output);

        if index + 1 < result.paths.len() {
            output.push('\n');
        }
    }

    output
}

struct PathFinder<'a> {
    graph: &'a IncludeGraph,
    target: FileId,
    limit: Option<usize>,
    paths: Vec<IncludePath>,
    stack: HashSet<FileId>,
    current_edges: Vec<IncludeEdge>,
}

impl<'a> PathFinder<'a> {
    fn new(graph: &'a IncludeGraph, target: FileId, limit: Option<usize>) -> Self {
        Self {
            graph,
            target,
            limit,
            paths: Vec::new(),
            stack: HashSet::new(),
            current_edges: Vec::new(),
        }
    }

    fn search(&mut self, file: FileId) {
        if self.reached_limit() {
            return;
        }

        self.stack.insert(file);

        for edge in sorted_resolved_edges_from(self.graph, file) {
            if self.reached_limit() {
                break;
            }

            let IncludeTarget::Resolved(next) = edge.to else {
                continue;
            };

            if self.stack.contains(&next) {
                continue;
            }

            self.current_edges.push(edge.clone());

            if next == self.target {
                self.paths.push(IncludePath {
                    edges: self.current_edges.clone(),
                });
            } else {
                self.search(next);
            }

            self.current_edges.pop();
        }

        self.stack.remove(&file);
    }

    fn reached_limit(&self) -> bool {
        self.limit
            .map(|limit| self.paths.len() >= limit)
            .unwrap_or(false)
    }
}

fn sorted_resolved_edges_from(graph: &IncludeGraph, file: FileId) -> Vec<IncludeEdge> {
    let mut edges = graph
        .edges_from(file)
        .filter(|edge| matches!(edge.to, IncludeTarget::Resolved(_)))
        .cloned()
        .collect::<Vec<_>>();

    edges.sort_by_key(|edge| {
        let to = match edge.to {
            IncludeTarget::Resolved(to) => to,
            IncludeTarget::External(_) | IncludeTarget::Missing(_) => FileId(usize::MAX),
        };

        (
            edge.line,
            display_file(graph, to, Path::new("")),
            edge.include_path.clone(),
        )
    });
    edges
}

fn render_path(graph: &IncludeGraph, path: &IncludePath, base_path: &Path, output: &mut String) {
    if path.edges.is_empty() {
        output.push_str("  <same file>\n");
        return;
    }

    for (index, edge) in path.edges.iter().enumerate() {
        if index == 0 {
            output.push_str(&format!(
                "{}:{}\n",
                display_file(graph, edge.from, base_path),
                edge.line
            ));
        }

        let IncludeTarget::Resolved(to) = edge.to else {
            continue;
        };

        output.push_str("  -> ");
        output.push_str(&display_file(graph, to, base_path));

        if let Some(next_edge) = path.edges.get(index + 1) {
            output.push_str(&format!(":{}", next_edge.line));
        }

        output.push('\n');
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileNode, IncludeKind};

    #[test]
    fn finds_a_single_include_path() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/app.h"),
                test_file(2, "/repo/include/config.h"),
            ],
            edges: vec![test_edge(0, 1, "app.h", 3), test_edge(1, 2, "config.h", 7)],
        };

        let result = find_include_paths(&graph, FileId(0), FileId(2), &WhyOptions::default());

        assert_eq!(result.paths.len(), 1);
        assert_eq!(
            render_why_result(&graph, &result, "/repo"),
            concat!(
                "src/main.cpp depends on include/config.h through 1 path.\n",
                "\n",
                "Path 1:\n",
                "src/main.cpp:3\n",
                "  -> include/app.h:7\n",
                "  -> include/config.h\n",
            )
        );
    }

    #[test]
    fn finds_multiple_paths_and_ignores_cycles() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/a.h"),
                test_file(2, "/repo/include/b.h"),
                test_file(3, "/repo/include/config.h"),
            ],
            edges: vec![
                test_edge(0, 1, "a.h", 1),
                test_edge(0, 2, "b.h", 2),
                test_edge(1, 2, "b.h", 3),
                test_edge(1, 3, "config.h", 4),
                test_edge(2, 1, "a.h", 5),
                test_edge(2, 3, "config.h", 6),
            ],
        };

        let result = find_include_paths(
            &graph,
            FileId(0),
            FileId(3),
            &WhyOptions {
                max_paths: None,
                shortest: false,
            },
        );

        assert_eq!(result.paths.len(), 4);
    }

    #[test]
    fn bounds_path_search() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/a.h"),
                test_file(2, "/repo/include/b.h"),
                test_file(3, "/repo/include/config.h"),
            ],
            edges: vec![
                test_edge(0, 1, "a.h", 1),
                test_edge(0, 2, "b.h", 2),
                test_edge(1, 3, "config.h", 3),
                test_edge(2, 3, "config.h", 4),
            ],
        };

        let result = find_include_paths(
            &graph,
            FileId(0),
            FileId(3),
            &WhyOptions {
                max_paths: Some(1),
                shortest: false,
            },
        );

        assert_eq!(result.paths.len(), 1);
        assert!(result.truncated);
    }

    #[test]
    fn returns_the_shortest_path() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/a.h"),
                test_file(2, "/repo/include/b.h"),
                test_file(3, "/repo/include/config.h"),
            ],
            edges: vec![
                test_edge(0, 1, "a.h", 1),
                test_edge(0, 3, "config.h", 2),
                test_edge(1, 2, "b.h", 3),
                test_edge(2, 3, "config.h", 4),
            ],
        };

        let result = find_include_paths(
            &graph,
            FileId(0),
            FileId(3),
            &WhyOptions {
                max_paths: None,
                shortest: true,
            },
        );

        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].edges.len(), 1);
    }

    #[test]
    fn reports_no_path() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/config.h"),
            ],
            edges: vec![],
        };

        let result = find_include_paths(&graph, FileId(0), FileId(1), &WhyOptions::default());

        assert!(result.paths.is_empty());
        assert_eq!(
            render_why_result(&graph, &result, "/repo"),
            "src/main.cpp does not depend on include/config.h.\n"
        );
    }

    fn test_file(id: usize, path: &str) -> FileNode {
        FileNode {
            id: FileId(id),
            path: PathBuf::from(path),
        }
    }

    fn test_edge(from: usize, to: usize, include_path: &str, line: usize) -> IncludeEdge {
        IncludeEdge {
            from: FileId(from),
            to: IncludeTarget::Resolved(FileId(to)),
            include_path: include_path.to_owned(),
            kind: IncludeKind::Quote,
            line,
        }
    }
}
