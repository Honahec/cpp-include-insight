use crate::{IncludeCycle, IncludeGraph, IncludeKind, IncludeTarget};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SnapshotOptions {
    pub absolute_paths: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IncludeGraphSnapshot {
    pub version: u32,
    pub root: SnapshotRoot,
    pub stats: SnapshotStats,
    pub files: Vec<SnapshotFile>,
    pub edges: Vec<SnapshotEdge>,
    pub cycles: Vec<SnapshotCycle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotRoot {
    pub path: String,
    pub path_style: SnapshotPathStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotPathStyle {
    ProjectRelative,
    Absolute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotStats {
    pub files: usize,
    pub edges: usize,
    pub resolved: usize,
    pub external: usize,
    pub missing: usize,
    pub cycles: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotFile {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotEdge {
    pub from: String,
    pub to: SnapshotTarget,
    pub include: String,
    pub kind: IncludeKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SnapshotTarget {
    Resolved { path: String },
    External { include: String },
    Missing { include: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotCycle {
    pub files: Vec<String>,
    pub edges: Vec<SnapshotCycleEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotCycleEdge {
    pub from: String,
    pub to: String,
    pub include: String,
    pub line: usize,
}

pub fn build_include_graph_snapshot(
    graph: &IncludeGraph,
    cycles: &[IncludeCycle],
    root: impl AsRef<Path>,
    options: SnapshotOptions,
) -> IncludeGraphSnapshot {
    let root = root.as_ref();
    let path_formatter = SnapshotPathFormatter::new(root, options.absolute_paths);
    let stats = graph.stats();
    let file_paths = graph
        .files
        .iter()
        .map(|file| (file.id, path_formatter.format(&file.path)))
        .collect::<HashMap<_, _>>();

    let mut files = file_paths
        .values()
        .cloned()
        .map(|path| SnapshotFile { path })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    let mut edges = graph
        .edges
        .iter()
        .map(|edge| SnapshotEdge {
            from: file_paths[&edge.from].clone(),
            to: match &edge.to {
                IncludeTarget::Resolved(to) => SnapshotTarget::Resolved {
                    path: file_paths[to].clone(),
                },
                IncludeTarget::External(include) => SnapshotTarget::External {
                    include: include.clone(),
                },
                IncludeTarget::Missing(include) => SnapshotTarget::Missing {
                    include: include.clone(),
                },
            },
            include: edge.include_path.clone(),
            kind: edge.kind,
            line: edge.line,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| edge_sort_key(left).cmp(&edge_sort_key(right)));

    let mut snapshot_cycles = cycles
        .iter()
        .map(|cycle| {
            let mut files = cycle
                .files
                .iter()
                .map(|file| file_paths[file].clone())
                .collect::<Vec<_>>();
            files.sort();

            let mut edges = cycle
                .edges
                .iter()
                .filter_map(|edge| {
                    let IncludeTarget::Resolved(to) = edge.to else {
                        return None;
                    };

                    Some(SnapshotCycleEdge {
                        from: file_paths[&edge.from].clone(),
                        to: file_paths[&to].clone(),
                        include: edge.include_path.clone(),
                        line: edge.line,
                    })
                })
                .collect::<Vec<_>>();
            edges.sort_by(|left, right| {
                (&left.from, left.line, &left.to, &left.include).cmp(&(
                    &right.from,
                    right.line,
                    &right.to,
                    &right.include,
                ))
            });

            SnapshotCycle { files, edges }
        })
        .collect::<Vec<_>>();
    snapshot_cycles.sort_by(|left, right| {
        left.files
            .first()
            .cmp(&right.files.first())
            .then_with(|| left.files.len().cmp(&right.files.len()))
    });

    IncludeGraphSnapshot {
        version: SNAPSHOT_VERSION,
        root: SnapshotRoot {
            path: if options.absolute_paths {
                absolutize(root).display().to_string()
            } else {
                ".".to_owned()
            },
            path_style: if options.absolute_paths {
                SnapshotPathStyle::Absolute
            } else {
                SnapshotPathStyle::ProjectRelative
            },
        },
        stats: SnapshotStats {
            files: files.len(),
            edges: edges.len(),
            resolved: stats.resolved,
            external: stats.external,
            missing: stats.missing,
            cycles: snapshot_cycles.len(),
        },
        files,
        edges,
        cycles: snapshot_cycles,
    }
}

fn edge_sort_key(edge: &SnapshotEdge) -> (&str, usize, u8, &str, &str) {
    let (target_rank, target) = match &edge.to {
        SnapshotTarget::Resolved { path } => (0, path.as_str()),
        SnapshotTarget::External { include } => (1, include.as_str()),
        SnapshotTarget::Missing { include } => (2, include.as_str()),
    };

    (
        edge.from.as_str(),
        edge.line,
        target_rank,
        target,
        edge.include.as_str(),
    )
}

struct SnapshotPathFormatter {
    root: PathBuf,
    absolute_paths: bool,
}

impl SnapshotPathFormatter {
    fn new(root: &Path, absolute_paths: bool) -> Self {
        Self {
            root: absolutize(root),
            absolute_paths,
        }
    }

    fn format(&self, path: &Path) -> String {
        let path = absolutize(path);

        if self.absolute_paths {
            return path.display().to_string();
        }

        relative_path(&self.root, &path).display().to_string()
    }
}

fn absolutize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|current_dir| current_dir.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        }
    })
}

fn relative_path(from: &Path, to: &Path) -> PathBuf {
    if let Ok(stripped) = to.strip_prefix(from) {
        return stripped.to_path_buf();
    }

    let from_components = normal_components(from);
    let to_components = normal_components(to);
    let common_len = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();

    let mut relative = PathBuf::new();

    for _ in common_len..from_components.len() {
        relative.push("..");
    }

    for component in &to_components[common_len..] {
        relative.push(component);
    }

    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn normal_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            Component::CurDir => None,
            Component::ParentDir => Some("..".to_owned()),
            Component::RootDir | Component::Prefix(_) => {
                Some(component.as_os_str().to_string_lossy().into_owned())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileId, FileNode, IncludeEdge};

    #[test]
    fn snapshot_uses_project_relative_paths_and_stable_ordering() {
        let graph = IncludeGraph {
            files: vec![
                test_file(0, "/repo/src/main.cpp"),
                test_file(1, "/repo/include/app.h"),
            ],
            edges: vec![
                test_edge(
                    0,
                    IncludeTarget::External("vector".to_owned()),
                    "vector",
                    IncludeKind::Angle,
                    2,
                ),
                test_edge(
                    0,
                    IncludeTarget::Resolved(FileId(1)),
                    "app.h",
                    IncludeKind::Quote,
                    1,
                ),
            ],
        };

        let snapshot =
            build_include_graph_snapshot(&graph, &[], "/repo", SnapshotOptions::default());

        assert_eq!(snapshot.root.path, ".");
        assert_eq!(snapshot.files[0].path, "include/app.h");
        assert_eq!(snapshot.files[1].path, "src/main.cpp");
        assert_eq!(snapshot.edges[0].include, "app.h");
        assert_eq!(snapshot.edges[1].include, "vector");
    }

    fn test_file(id: usize, path: &str) -> FileNode {
        FileNode {
            id: FileId(id),
            path: PathBuf::from(path),
        }
    }

    fn test_edge(
        from: usize,
        to: IncludeTarget,
        include_path: &str,
        kind: IncludeKind,
        line: usize,
    ) -> IncludeEdge {
        IncludeEdge {
            from: FileId(from),
            to,
            include_path: include_path.to_owned(),
            kind,
            line,
        }
    }
}
