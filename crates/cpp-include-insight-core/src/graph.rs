use crate::{
    IncludeKind,
    resolver::{IncludeResolution, IncludeResolver},
    scanner::ScanResult,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileNode {
    pub id: FileId,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncludeTarget {
    Resolved(FileId),
    External(String),
    Missing(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncludeEdge {
    pub from: FileId,
    pub to: IncludeTarget,
    pub include_path: String,
    pub kind: IncludeKind,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncludeGraph {
    pub files: Vec<FileNode>,
    pub edges: Vec<IncludeEdge>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncludeGraphStats {
    pub resolved: usize,
    pub external: usize,
    pub missing: usize,
}

impl IncludeGraph {
    pub fn from_scan_result(scan: &ScanResult, resolver: &IncludeResolver) -> Self {
        let mut graph = Self::default();
        let mut file_ids = HashMap::new();

        for file in &scan.files {
            graph.get_or_add_file_id(&file.file, &mut file_ids);
        }

        for file in &scan.files {
            let from = graph.get_or_add_file_id(&file.file, &mut file_ids);

            for include in &file.includes {
                for resolution in resolver.resolve_includes(&file.file, include) {
                    let to = match resolution {
                        IncludeResolution::Resolved(path) => {
                            IncludeTarget::Resolved(graph.get_or_add_file_id(&path, &mut file_ids))
                        }
                        IncludeResolution::External(path) => IncludeTarget::External(path),
                        IncludeResolution::Missing(path) => IncludeTarget::Missing(path),
                    };

                    graph.edges.push(IncludeEdge {
                        from,
                        to,
                        include_path: include.path.clone(),
                        kind: include.kind,
                        line: include.line,
                    });
                }
            }
        }

        graph
    }

    pub fn file(&self, id: FileId) -> Option<&FileNode> {
        self.files.get(id.0)
    }

    pub fn file_id_for_path(&self, path: &Path) -> Option<FileId> {
        let needle = normalize_path(path);

        self.files
            .iter()
            .find(|file| normalize_path(&file.path) == needle)
            .map(|file| file.id)
    }

    pub fn edges_from(&self, id: FileId) -> impl Iterator<Item = &IncludeEdge> {
        self.edges.iter().filter(move |edge| edge.from == id)
    }

    pub fn resolved_edges(&self) -> impl Iterator<Item = &IncludeEdge> {
        self.edges
            .iter()
            .filter(|edge| matches!(edge.to, IncludeTarget::Resolved(_)))
    }

    pub fn external_edges(&self) -> impl Iterator<Item = &IncludeEdge> {
        self.edges
            .iter()
            .filter(|edge| matches!(edge.to, IncludeTarget::External(_)))
    }

    pub fn missing_edges(&self) -> impl Iterator<Item = &IncludeEdge> {
        self.edges
            .iter()
            .filter(|edge| matches!(edge.to, IncludeTarget::Missing(_)))
    }

    pub fn stats(&self) -> IncludeGraphStats {
        IncludeGraphStats {
            resolved: self.resolved_edges().count(),
            external: self.external_edges().count(),
            missing: self.missing_edges().count(),
        }
    }

    fn get_or_add_file_id(
        &mut self,
        path: &Path,
        file_ids: &mut HashMap<PathBuf, FileId>,
    ) -> FileId {
        let key = normalize_path(path);

        if let Some(id) = file_ids.get(&key) {
            return *id;
        }

        let id = FileId(self.files.len());
        self.files.push(FileNode {
            id,
            path: path.to_path_buf(),
        });
        file_ids.insert(key, id);

        id
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parser::parse_include_line, scanner::FileIncludes};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn builds_file_nodes_and_resolved_include_edges() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        let app_h = src_dir.join("app.h");
        fs::write(&main_cpp, r#"#include "app.h""#).unwrap();
        fs::write(&app_h, "").unwrap();

        let scan = ScanResult {
            files_scanned: 2,
            includes_found: 1,
            files: vec![
                FileIncludes {
                    file: main_cpp.clone(),
                    includes: vec![parse_include_line(r#"#include "app.h""#, 3).unwrap()],
                },
                FileIncludes {
                    file: app_h.clone(),
                    includes: vec![],
                },
            ],
        };
        let resolver = IncludeResolver::new(temp.path(), vec![]);

        let graph = IncludeGraph::from_scan_result(&scan, &resolver);

        assert_eq!(graph.files.len(), 2);
        assert_eq!(graph.files[0].path, main_cpp);
        assert_eq!(graph.files[1].path, app_h);
        assert_eq!(
            graph.edges,
            vec![IncludeEdge {
                from: FileId(0),
                to: IncludeTarget::Resolved(FileId(1)),
                include_path: "app.h".to_owned(),
                kind: IncludeKind::Quote,
                line: 3,
            }]
        );
    }

    #[test]
    fn preserves_external_include_targets() {
        let temp = tempdir().unwrap();
        let main_cpp = temp.path().join("main.cpp");
        fs::write(&main_cpp, "#include <vector>").unwrap();

        let scan = ScanResult {
            files_scanned: 1,
            includes_found: 1,
            files: vec![FileIncludes {
                file: main_cpp,
                includes: vec![parse_include_line("#include <vector>", 7).unwrap()],
            }],
        };
        let resolver = IncludeResolver::new(temp.path(), vec![]);

        let graph = IncludeGraph::from_scan_result(&scan, &resolver);

        assert_eq!(
            graph.edges,
            vec![IncludeEdge {
                from: FileId(0),
                to: IncludeTarget::External("vector".to_owned()),
                include_path: "vector".to_owned(),
                kind: IncludeKind::Angle,
                line: 7,
            }]
        );
    }

    #[test]
    fn preserves_missing_include_targets() {
        let temp = tempdir().unwrap();
        let main_cpp = temp.path().join("main.cpp");
        fs::write(&main_cpp, r#"#include "missing.h""#).unwrap();

        let scan = ScanResult {
            files_scanned: 1,
            includes_found: 1,
            files: vec![FileIncludes {
                file: main_cpp,
                includes: vec![parse_include_line(r#"#include "missing.h""#, 11).unwrap()],
            }],
        };
        let resolver = IncludeResolver::new(temp.path(), vec![]);

        let graph = IncludeGraph::from_scan_result(&scan, &resolver);

        assert_eq!(
            graph.edges,
            vec![IncludeEdge {
                from: FileId(0),
                to: IncludeTarget::Missing("missing.h".to_owned()),
                include_path: "missing.h".to_owned(),
                kind: IncludeKind::Quote,
                line: 11,
            }]
        );
    }

    #[test]
    fn can_iterate_edges_from_a_file() {
        let graph = IncludeGraph {
            files: vec![FileNode {
                id: FileId(0),
                path: PathBuf::from("main.cpp"),
            }],
            edges: vec![IncludeEdge {
                from: FileId(0),
                to: IncludeTarget::External("vector".to_owned()),
                include_path: "vector".to_owned(),
                kind: IncludeKind::Angle,
                line: 1,
            }],
        };

        assert_eq!(
            graph.file(FileId(0)).unwrap().path,
            PathBuf::from("main.cpp")
        );
        assert_eq!(graph.edges_from(FileId(0)).count(), 1);
    }

    #[test]
    fn counts_edges_by_target_kind() {
        let graph = IncludeGraph {
            files: vec![],
            edges: vec![
                IncludeEdge {
                    from: FileId(0),
                    to: IncludeTarget::Resolved(FileId(1)),
                    include_path: "app.h".to_owned(),
                    kind: IncludeKind::Quote,
                    line: 1,
                },
                IncludeEdge {
                    from: FileId(0),
                    to: IncludeTarget::External("vector".to_owned()),
                    include_path: "vector".to_owned(),
                    kind: IncludeKind::Angle,
                    line: 2,
                },
                IncludeEdge {
                    from: FileId(0),
                    to: IncludeTarget::Missing("missing.h".to_owned()),
                    include_path: "missing.h".to_owned(),
                    kind: IncludeKind::Quote,
                    line: 3,
                },
            ],
        };

        assert_eq!(
            graph.stats(),
            IncludeGraphStats {
                resolved: 1,
                external: 1,
                missing: 1,
            }
        );
    }

    #[test]
    fn finds_file_id_by_equivalent_path() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        fs::write(&main_cpp, "").unwrap();

        let graph = IncludeGraph {
            files: vec![FileNode {
                id: FileId(0),
                path: main_cpp.clone(),
            }],
            edges: vec![],
        };

        assert_eq!(
            graph.file_id_for_path(&src_dir.join("../src/main.cpp")),
            Some(FileId(0))
        );
    }
}
