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
    let base_path = base_path.as_ref();
    let mut output = String::new();
    let mut shown = HashSet::new();
    let mut stack = HashSet::new();

    output.push_str(&display_file(graph, root, base_path));
    output.push('\n');

    shown.insert(root);
    stack.insert(root);
    render_children(
        graph,
        root,
        base_path,
        "",
        &mut shown,
        &mut stack,
        &mut output,
    );

    output
}

fn render_children(
    graph: &IncludeGraph,
    parent: FileId,
    base_path: &Path,
    prefix: &str,
    shown: &mut HashSet<FileId>,
    stack: &mut HashSet<FileId>,
    output: &mut String,
) {
    let children = graph
        .edges_from(parent)
        .filter_map(|edge| match edge.to {
            IncludeTarget::Resolved(child) => Some(child),
            IncludeTarget::External(_) | IncludeTarget::Missing(_) => None,
        })
        .collect::<Vec<_>>();

    for (index, child) in children.iter().copied().enumerate() {
        let is_last = index + 1 == children.len();
        let branch = if is_last { "`-- " } else { "|-- " };
        let next_prefix = if is_last { "    " } else { "|   " };

        output.push_str(prefix);
        output.push_str(branch);
        output.push_str(&display_file(graph, child, base_path));

        if stack.contains(&child) {
            output.push_str(" [cycle]\n");
            continue;
        }

        if shown.contains(&child) {
            output.push_str(" [already shown]\n");
            continue;
        }

        output.push('\n');
        shown.insert(child);
        stack.insert(child);

        let child_prefix = format!("{prefix}{next_prefix}");
        render_children(graph, child, base_path, &child_prefix, shown, stack, output);

        stack.remove(&child);
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
