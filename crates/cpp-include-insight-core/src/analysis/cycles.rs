use crate::{FileId, IncludeEdge, IncludeGraph, IncludeTarget};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeCycle {
    pub files: Vec<FileId>,
    pub edges: Vec<IncludeEdge>,
}

pub fn detect_include_cycles(graph: &IncludeGraph) -> Vec<IncludeCycle> {
    let mut finder = TarjanCycleFinder::new(graph);
    let components = finder.find_components();
    let mut cycles = components
        .into_iter()
        .filter_map(|component| cycle_from_component(graph, component))
        .collect::<Vec<_>>();

    cycles.sort_by(|left, right| {
        cycle_sort_key(graph, left)
            .cmp(&cycle_sort_key(graph, right))
            .then_with(|| left.files.len().cmp(&right.files.len()))
    });

    cycles
}

pub fn render_include_cycles(
    graph: &IncludeGraph,
    cycles: &[IncludeCycle],
    base_path: impl AsRef<Path>,
) -> String {
    let base_path = base_path.as_ref();
    let mut output = String::new();
    let suffix = if cycles.len() == 1 { "cycle" } else { "cycles" };

    output.push_str(&format!("Found {} include {suffix}.", cycles.len()));
    output.push('\n');

    if cycles.is_empty() {
        return output;
    }

    output.push('\n');

    for (index, cycle) in cycles.iter().enumerate() {
        output.push_str(&format!("Cycle {}:\n", index + 1));
        render_cycle_path(graph, cycle, base_path, &mut output);

        if index + 1 < cycles.len() {
            output.push('\n');
        }
    }

    output
}

struct TarjanCycleFinder<'a> {
    graph: &'a IncludeGraph,
    adjacency: HashMap<FileId, Vec<FileId>>,
    next_index: usize,
    stack: Vec<FileId>,
    on_stack: HashSet<FileId>,
    index_by_file: HashMap<FileId, usize>,
    lowlink_by_file: HashMap<FileId, usize>,
    components: Vec<Vec<FileId>>,
}

impl<'a> TarjanCycleFinder<'a> {
    fn new(graph: &'a IncludeGraph) -> Self {
        let mut adjacency: HashMap<FileId, Vec<FileId>> = HashMap::new();

        for edge in graph.resolved_edges() {
            let IncludeTarget::Resolved(to) = edge.to else {
                continue;
            };

            let children = adjacency.entry(edge.from).or_default();
            if !children.contains(&to) {
                children.push(to);
            }
        }

        for children in adjacency.values_mut() {
            children.sort_by_key(|file| file.0);
        }

        Self {
            graph,
            adjacency,
            next_index: 0,
            stack: Vec::new(),
            on_stack: HashSet::new(),
            index_by_file: HashMap::new(),
            lowlink_by_file: HashMap::new(),
            components: Vec::new(),
        }
    }

    fn find_components(&mut self) -> Vec<Vec<FileId>> {
        for file in &self.graph.files {
            if !self.index_by_file.contains_key(&file.id) {
                self.connect(file.id);
            }
        }

        std::mem::take(&mut self.components)
    }

    fn connect(&mut self, file: FileId) {
        self.index_by_file.insert(file, self.next_index);
        self.lowlink_by_file.insert(file, self.next_index);
        self.next_index += 1;
        self.stack.push(file);
        self.on_stack.insert(file);

        for child in self.adjacency.get(&file).cloned().unwrap_or_default() {
            if !self.index_by_file.contains_key(&child) {
                self.connect(child);
                let child_lowlink = self.lowlink_by_file[&child];
                let file_lowlink = self.lowlink_by_file[&file];
                self.lowlink_by_file
                    .insert(file, file_lowlink.min(child_lowlink));
            } else if self.on_stack.contains(&child) {
                let child_index = self.index_by_file[&child];
                let file_lowlink = self.lowlink_by_file[&file];
                self.lowlink_by_file
                    .insert(file, file_lowlink.min(child_index));
            }
        }

        if self.lowlink_by_file[&file] == self.index_by_file[&file] {
            let mut component = Vec::new();

            while let Some(member) = self.stack.pop() {
                self.on_stack.remove(&member);
                component.push(member);

                if member == file {
                    break;
                }
            }

            self.components.push(component);
        }
    }
}

fn cycle_from_component(graph: &IncludeGraph, mut files: Vec<FileId>) -> Option<IncludeCycle> {
    let file_set = files.iter().copied().collect::<HashSet<_>>();
    let mut edges = graph
        .resolved_edges()
        .filter_map(|edge| {
            let IncludeTarget::Resolved(to) = edge.to else {
                return None;
            };

            if file_set.contains(&edge.from) && file_set.contains(&to) {
                Some(edge.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if files.len() == 1 {
        let file = files[0];
        let has_self_loop = edges.iter().any(|edge| {
            matches!(edge.to, IncludeTarget::Resolved(to) if edge.from == file && to == file)
        });

        if !has_self_loop {
            return None;
        }
    }

    files.sort_by_key(|file| file_sort_key(graph, *file));
    edges.sort_by_key(|edge| edge_sort_key(graph, edge));

    Some(IncludeCycle { files, edges })
}

fn render_cycle_path(
    graph: &IncludeGraph,
    cycle: &IncludeCycle,
    base_path: &Path,
    output: &mut String,
) {
    let Some(path) = find_cycle_path(cycle) else {
        for edge in &cycle.edges {
            output.push_str("  ");
            output.push_str(&display_path_edge(graph, edge, base_path));
            output.push('\n');
        }
        return;
    };

    for (index, edge) in path.iter().enumerate() {
        if index == 0 {
            output.push_str(&format!(
                "{}:{}\n",
                display_file(graph, edge.from, base_path),
                edge.line
            ));
        }

        let Some(to) = resolved_target(edge) else {
            continue;
        };

        output.push_str("  -> ");
        output.push_str(&display_file(graph, to, base_path));

        if let Some(next_edge) = path.get(index + 1) {
            output.push_str(&format!(":{}", next_edge.line));
        }

        output.push('\n');
    }
}

fn find_cycle_path(cycle: &IncludeCycle) -> Option<Vec<&IncludeEdge>> {
    let start = *cycle.files.first()?;
    let mut stack = HashSet::from([start]);
    let mut path = Vec::new();

    if find_cycle_path_from(cycle, start, start, &mut stack, &mut path) {
        Some(path)
    } else {
        None
    }
}

fn find_cycle_path_from<'a>(
    cycle: &'a IncludeCycle,
    start: FileId,
    current: FileId,
    stack: &mut HashSet<FileId>,
    path: &mut Vec<&'a IncludeEdge>,
) -> bool {
    for edge in cycle.edges.iter().filter(|edge| edge.from == current) {
        let Some(next) = resolved_target(edge) else {
            continue;
        };

        path.push(edge);

        if next == start {
            return true;
        }

        if !stack.contains(&next) {
            stack.insert(next);

            if find_cycle_path_from(cycle, start, next, stack, path) {
                return true;
            }

            stack.remove(&next);
        }

        path.pop();
    }

    false
}

fn resolved_target(edge: &IncludeEdge) -> Option<FileId> {
    match edge.to {
        IncludeTarget::Resolved(to) => Some(to),
        IncludeTarget::External(_) | IncludeTarget::Missing(_) => None,
    }
}

fn cycle_sort_key(graph: &IncludeGraph, cycle: &IncludeCycle) -> String {
    cycle
        .files
        .first()
        .map(|file| file_sort_key(graph, *file))
        .unwrap_or_default()
}

fn edge_sort_key(graph: &IncludeGraph, edge: &IncludeEdge) -> (String, usize, String, String) {
    let to = match edge.to {
        IncludeTarget::Resolved(to) => to,
        IncludeTarget::External(_) | IncludeTarget::Missing(_) => FileId(usize::MAX),
    };

    (
        file_sort_key(graph, edge.from),
        edge.line,
        file_sort_key(graph, to),
        edge.include_path.clone(),
    )
}

fn file_sort_key(graph: &IncludeGraph, id: FileId) -> String {
    graph
        .file(id)
        .map(|file| file.path.display().to_string())
        .unwrap_or_else(|| format!("<unknown:{}>", id.0))
}

fn display_path_edge(graph: &IncludeGraph, edge: &IncludeEdge, base_path: &Path) -> String {
    let to = match edge.to {
        IncludeTarget::Resolved(to) => display_file(graph, to, base_path),
        IncludeTarget::External(_) | IncludeTarget::Missing(_) => "<unresolved>".to_owned(),
    };

    format!(
        "{}:{} -> {}",
        display_file(graph, edge.from, base_path),
        edge.line,
        to
    )
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
    fn reports_no_cycles_for_acyclic_graph() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/app.h"),
            ],
            edges: vec![test_edge(0, 1, "app.h", 3)],
        };

        assert!(detect_include_cycles(&graph).is_empty());
        assert_eq!(
            render_include_cycles(&graph, &detect_include_cycles(&graph), "/repo"),
            "Found 0 include cycles.\n"
        );
    }

    #[test]
    fn detects_single_cycle_with_internal_edges() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/a.h"),
                test_file(2, "/repo/include/b.h"),
            ],
            edges: vec![
                test_edge(0, 1, "a.h", 1),
                test_edge(1, 2, "b.h", 4),
                test_edge(2, 1, "a.h", 7),
            ],
        };

        let cycles = detect_include_cycles(&graph);

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].files, vec![FileId(1), FileId(2)]);
        assert_eq!(cycles[0].edges.len(), 2);
        assert_eq!(
            render_include_cycles(&graph, &cycles, "/repo"),
            concat!(
                "Found 1 include cycle.\n",
                "\n",
                "Cycle 1:\n",
                "include/a.h:4\n",
                "  -> include/b.h:7\n",
                "  -> include/a.h\n",
            )
        );
    }

    #[test]
    fn detects_multiple_cycles_and_ignores_unresolved_edges() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/include/a.h"),
                test_file(1, "/repo/include/b.h"),
                test_file(2, "/repo/include/c.h"),
                test_file(3, "/repo/include/d.h"),
            ],
            edges: vec![
                test_edge(0, 1, "b.h", 1),
                test_edge(1, 0, "a.h", 2),
                test_edge(2, 3, "d.h", 3),
                test_edge(3, 2, "c.h", 4),
                IncludeEdge {
                    from: FileId(0),
                    to: IncludeTarget::External("vector".to_owned()),
                    include_path: "vector".to_owned(),
                    kind: IncludeKind::Angle,
                    line: 5,
                },
                IncludeEdge {
                    from: FileId(1),
                    to: IncludeTarget::Missing("missing.h".to_owned()),
                    include_path: "missing.h".to_owned(),
                    kind: IncludeKind::Quote,
                    line: 6,
                },
            ],
        };

        let cycles = detect_include_cycles(&graph);

        assert_eq!(cycles.len(), 2);
        assert_eq!(cycles[0].files, vec![FileId(0), FileId(1)]);
        assert_eq!(cycles[1].files, vec![FileId(2), FileId(3)]);
    }

    #[test]
    fn detects_self_include_cycle() {
        let graph = IncludeGraph {
            files: vec![test_file(0, "/repo/include/self.h")],
            edges: vec![test_edge(0, 0, "self.h", 9)],
        };

        let cycles = detect_include_cycles(&graph);

        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].files, vec![FileId(0)]);
        assert_eq!(cycles[0].edges.len(), 1);
        assert_eq!(
            render_include_cycles(&graph, &cycles, "/repo"),
            concat!(
                "Found 1 include cycle.\n",
                "\n",
                "Cycle 1:\n",
                "include/self.h:9\n",
                "  -> include/self.h\n",
            )
        );
    }

    #[test]
    fn renders_one_followable_path_for_a_branching_component() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/include/a.h"),
                test_file(1, "/repo/include/b.h"),
                test_file(2, "/repo/include/c.h"),
            ],
            edges: vec![
                test_edge(0, 1, "b.h", 1),
                test_edge(0, 2, "c.h", 2),
                test_edge(1, 0, "a.h", 3),
                test_edge(2, 0, "a.h", 4),
            ],
        };

        let cycles = detect_include_cycles(&graph);

        assert_eq!(
            render_include_cycles(&graph, &cycles, "/repo"),
            concat!(
                "Found 1 include cycle.\n",
                "\n",
                "Cycle 1:\n",
                "include/a.h:1\n",
                "  -> include/b.h:3\n",
                "  -> include/a.h\n",
            )
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
