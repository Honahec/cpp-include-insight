use crate::{FileId, IncludeGraph, IncludeTarget};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub fn render_include_tree(
    graph: &IncludeGraph,
    root: FileId,
    base_path: impl AsRef<Path>,
) -> String {
    render_tree(graph, root, base_path, TreeDirection::Forward)
}

pub fn render_reverse_include_tree(
    graph: &IncludeGraph,
    root: FileId,
    base_path: impl AsRef<Path>,
) -> String {
    render_tree(graph, root, base_path, TreeDirection::Reverse)
}

#[derive(Debug, Clone, Copy)]
enum TreeDirection {
    Forward,
    Reverse,
}

fn render_tree(
    graph: &IncludeGraph,
    root: FileId,
    base_path: impl AsRef<Path>,
    direction: TreeDirection,
) -> String {
    let base_path = base_path.as_ref();
    let mut renderer = TreeRenderer {
        graph,
        base_path,
        direction,
        output: String::new(),
        shown: HashSet::new(),
        stack: HashSet::new(),
    };

    renderer.render(root);
    renderer.output
}

struct TreeRenderer<'a> {
    graph: &'a IncludeGraph,
    base_path: &'a Path,
    direction: TreeDirection,
    output: String,
    shown: HashSet<FileId>,
    stack: HashSet<FileId>,
}

impl TreeRenderer<'_> {
    fn render(&mut self, root: FileId) {
        self.output
            .push_str(&display_file(self.graph, root, self.base_path));
        self.output.push('\n');

        self.shown.insert(root);
        self.stack.insert(root);
        self.render_children(root, "");
    }

    fn render_children(&mut self, parent: FileId, prefix: &str) {
        let children = self.children(parent);

        for (index, child) in children.iter().copied().enumerate() {
            let is_last = index + 1 == children.len();
            let branch = if is_last { "`-- " } else { "|-- " };
            let next_prefix = if is_last { "    " } else { "|   " };

            self.output.push_str(prefix);
            self.output.push_str(branch);
            self.output
                .push_str(&display_file(self.graph, child, self.base_path));

            if self.stack.contains(&child) {
                self.output.push_str(" [cycle]\n");
                continue;
            }

            if self.shown.contains(&child) {
                self.output.push_str(" [already shown]\n");
                continue;
            }

            self.output.push('\n');
            self.shown.insert(child);
            self.stack.insert(child);

            let child_prefix = format!("{prefix}{next_prefix}");
            self.render_children(child, &child_prefix);

            self.stack.remove(&child);
        }
    }

    fn children(&self, parent: FileId) -> Vec<FileId> {
        let mut children = match self.direction {
            TreeDirection::Forward => self
                .graph
                .edges
                .iter()
                .filter_map(|edge| {
                    if edge.from != parent {
                        return None;
                    }

                    match edge.to {
                        IncludeTarget::Resolved(child) => Some(child),
                        IncludeTarget::External(_) | IncludeTarget::Missing(_) => None,
                    }
                })
                .collect::<Vec<_>>(),
            TreeDirection::Reverse => self
                .graph
                .edges
                .iter()
                .filter_map(|edge| match edge.to {
                    IncludeTarget::Resolved(child) if child == parent => Some(edge.from),
                    IncludeTarget::Resolved(_)
                    | IncludeTarget::External(_)
                    | IncludeTarget::Missing(_) => None,
                })
                .collect::<Vec<_>>(),
        };

        if matches!(self.direction, TreeDirection::Reverse) {
            children.sort_by_key(|child| display_file(self.graph, *child, self.base_path));
        }

        children
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
    use crate::{FileNode, IncludeEdge, IncludeKind};

    #[test]
    fn renders_forward_include_tree() {
        let graph = IncludeGraph {
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
                IncludeEdge {
                    from: FileId(0),
                    to: IncludeTarget::Resolved(FileId(1)),
                    include_path: "app.h".to_owned(),
                    kind: IncludeKind::Quote,
                    line: 1,
                },
                IncludeEdge {
                    from: FileId(1),
                    to: IncludeTarget::Resolved(FileId(2)),
                    include_path: "config.h".to_owned(),
                    kind: IncludeKind::Quote,
                    line: 2,
                },
            ],
        };

        assert_eq!(
            render_include_tree(&graph, FileId(0), "/repo"),
            "src/main.cpp\n`-- include/app.h\n    `-- include/config.h\n"
        );
    }

    #[test]
    fn marks_repeated_nodes() {
        let graph = IncludeGraph {
            files: vec![
                FileNode {
                    id: FileId(0),
                    path: PathBuf::from("/repo/src/main.cpp"),
                },
                FileNode {
                    id: FileId(1),
                    path: PathBuf::from("/repo/include/a.h"),
                },
                FileNode {
                    id: FileId(2),
                    path: PathBuf::from("/repo/include/b.h"),
                },
                FileNode {
                    id: FileId(3),
                    path: PathBuf::from("/repo/include/shared.h"),
                },
            ],
            edges: vec![
                test_edge(0, 1, "a.h"),
                test_edge(0, 2, "b.h"),
                test_edge(1, 3, "shared.h"),
                test_edge(2, 3, "shared.h"),
            ],
        };

        assert_eq!(
            render_include_tree(&graph, FileId(0), "/repo"),
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
    fn marks_cycles() {
        let graph = IncludeGraph {
            files: vec![
                FileNode {
                    id: FileId(0),
                    path: PathBuf::from("/repo/src/main.cpp"),
                },
                FileNode {
                    id: FileId(1),
                    path: PathBuf::from("/repo/include/a.h"),
                },
                FileNode {
                    id: FileId(2),
                    path: PathBuf::from("/repo/include/b.h"),
                },
            ],
            edges: vec![
                test_edge(0, 1, "a.h"),
                test_edge(1, 2, "b.h"),
                test_edge(2, 1, "a.h"),
            ],
        };

        assert_eq!(
            render_include_tree(&graph, FileId(0), "/repo"),
            concat!(
                "src/main.cpp\n",
                "`-- include/a.h\n",
                "    `-- include/b.h\n",
                "        `-- include/a.h [cycle]\n",
            )
        );
    }

    #[test]
    fn renders_reverse_include_tree() {
        let graph = IncludeGraph {
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
            edges: vec![test_edge(0, 1, "app.h"), test_edge(1, 2, "config.h")],
        };

        assert_eq!(
            render_reverse_include_tree(&graph, FileId(2), "/repo"),
            "include/config.h\n`-- include/app.h\n    `-- src/main.cpp\n"
        );
    }

    #[test]
    fn reverse_tree_marks_repeated_nodes() {
        let graph = IncludeGraph {
            files: vec![
                FileNode {
                    id: FileId(0),
                    path: PathBuf::from("/repo/src/main.cpp"),
                },
                FileNode {
                    id: FileId(1),
                    path: PathBuf::from("/repo/include/a.h"),
                },
                FileNode {
                    id: FileId(2),
                    path: PathBuf::from("/repo/include/b.h"),
                },
                FileNode {
                    id: FileId(3),
                    path: PathBuf::from("/repo/include/shared.h"),
                },
            ],
            edges: vec![
                test_edge(0, 1, "a.h"),
                test_edge(0, 2, "b.h"),
                test_edge(1, 3, "shared.h"),
                test_edge(2, 3, "shared.h"),
            ],
        };

        assert_eq!(
            render_reverse_include_tree(&graph, FileId(3), "/repo"),
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
    fn reverse_tree_marks_cycles() {
        let graph = IncludeGraph {
            files: vec![
                FileNode {
                    id: FileId(0),
                    path: PathBuf::from("/repo/src/main.cpp"),
                },
                FileNode {
                    id: FileId(1),
                    path: PathBuf::from("/repo/include/a.h"),
                },
                FileNode {
                    id: FileId(2),
                    path: PathBuf::from("/repo/include/b.h"),
                },
            ],
            edges: vec![
                test_edge(0, 1, "a.h"),
                test_edge(1, 2, "b.h"),
                test_edge(2, 1, "a.h"),
            ],
        };

        assert_eq!(
            render_reverse_include_tree(&graph, FileId(1), "/repo"),
            concat!(
                "include/a.h\n",
                "|-- include/b.h\n",
                "|   `-- include/a.h [cycle]\n",
                "`-- src/main.cpp\n",
            )
        );
    }

    fn test_edge(from: usize, to: usize, include_path: &str) -> IncludeEdge {
        IncludeEdge {
            from: FileId(from),
            to: IncludeTarget::Resolved(FileId(to)),
            include_path: include_path.to_owned(),
            kind: IncludeKind::Quote,
            line: 1,
        }
    }
}
