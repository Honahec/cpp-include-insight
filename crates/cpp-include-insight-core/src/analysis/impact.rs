use crate::{FileId, IncludeGraph, IncludeTarget};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    TranslationUnit,
    Header,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactResult {
    pub target: FileId,
    pub all_dependants: Vec<FileId>,
    pub translation_units: Vec<FileId>,
    pub headers: Vec<FileId>,
    pub direct_dependants: Vec<FileId>,
    pub transitive_dependants: Vec<FileId>,
}

pub fn analyze_include_impact(graph: &IncludeGraph, target: FileId) -> ImpactResult {
    let direct_dependants = sorted_file_ids(graph, reverse_dependants(graph, target));
    let direct_set = direct_dependants.iter().copied().collect::<HashSet<_>>();

    let mut seen = HashSet::from([target]);
    let mut queue = VecDeque::new();

    for dependant in &direct_dependants {
        if seen.insert(*dependant) {
            queue.push_back(*dependant);
        }
    }

    while let Some(file) = queue.pop_front() {
        for dependant in reverse_dependants(graph, file) {
            if seen.insert(dependant) {
                queue.push_back(dependant);
            }
        }
    }

    let all_dependants = sorted_file_ids(
        graph,
        seen.into_iter()
            .filter(|file| *file != target)
            .collect::<Vec<_>>(),
    );
    let transitive_dependants = all_dependants
        .iter()
        .copied()
        .filter(|file| !direct_set.contains(file))
        .collect::<Vec<_>>();
    let translation_units = all_dependants
        .iter()
        .copied()
        .filter(|file| {
            graph
                .file(*file)
                .map(|file| classify_file(&file.path) == FileKind::TranslationUnit)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let headers = all_dependants
        .iter()
        .copied()
        .filter(|file| {
            graph
                .file(*file)
                .map(|file| classify_file(&file.path) == FileKind::Header)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    ImpactResult {
        target,
        all_dependants,
        translation_units,
        headers,
        direct_dependants,
        transitive_dependants,
    }
}

pub fn classify_file(path: impl AsRef<Path>) -> FileKind {
    match path
        .as_ref()
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("c" | "cc" | "cpp" | "cxx") => FileKind::TranslationUnit,
        Some("h" | "hh" | "hpp" | "hxx") => FileKind::Header,
        Some(_) | None => FileKind::Other,
    }
}

pub fn render_impact_result(
    graph: &IncludeGraph,
    result: &ImpactResult,
    base_path: impl AsRef<Path>,
) -> String {
    let base_path = base_path.as_ref();
    let target = display_file(graph, result.target, base_path);
    let suffix = if result.all_dependants.len() == 1 {
        "file"
    } else {
        "files"
    };
    let mut output = format!(
        "{target} impacts {} {suffix}.\n",
        result.all_dependants.len()
    );

    render_section(
        graph,
        "Translation units",
        &result.translation_units,
        base_path,
        &mut output,
    );
    render_section(graph, "Headers", &result.headers, base_path, &mut output);
    render_section(
        graph,
        "Direct dependants",
        &result.direct_dependants,
        base_path,
        &mut output,
    );
    render_section(
        graph,
        "Transitive dependants",
        &result.transitive_dependants,
        base_path,
        &mut output,
    );

    output
}

fn reverse_dependants(graph: &IncludeGraph, target: FileId) -> Vec<FileId> {
    graph
        .resolved_edges()
        .filter_map(|edge| match edge.to {
            IncludeTarget::Resolved(to) if to == target && edge.from != target => Some(edge.from),
            IncludeTarget::Resolved(_) | IncludeTarget::External(_) | IncludeTarget::Missing(_) => {
                None
            }
        })
        .collect()
}

fn sorted_file_ids(graph: &IncludeGraph, files: Vec<FileId>) -> Vec<FileId> {
    let mut unique = files
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    unique.sort_by_key(|file| display_file(graph, *file, Path::new("")));
    unique
}

fn render_section(
    graph: &IncludeGraph,
    title: &str,
    files: &[FileId],
    base_path: &Path,
    output: &mut String,
) {
    output.push('\n');
    output.push_str(title);
    output.push_str(":\n");

    if files.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for file in files {
        output.push_str("  ");
        output.push_str(&display_file(graph, *file, base_path));
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
    use crate::{FileNode, IncludeEdge, IncludeKind};

    #[test]
    fn reports_direct_transitive_and_classified_dependants() {
        let graph = IncludeGraph {
            files: vec![
                file(0, "/repo/include/config.h"),
                file(1, "/repo/include/app.h"),
                file(2, "/repo/src/main.cpp"),
                file(3, "/repo/include/logger.hpp"),
                file(4, "/repo/src/server.cxx"),
            ],
            edges: vec![
                edge(1, 0, "config.h"),
                edge(2, 1, "app.h"),
                edge(3, 0, "config.h"),
                edge(4, 3, "logger.hpp"),
            ],
        };

        let impact = analyze_include_impact(&graph, FileId(0));

        assert_eq!(impact.direct_dependants, vec![FileId(1), FileId(3)]);
        assert_eq!(impact.transitive_dependants, vec![FileId(2), FileId(4)]);
        assert_eq!(impact.translation_units, vec![FileId(2), FileId(4)]);
        assert_eq!(impact.headers, vec![FileId(1), FileId(3)]);
        assert_eq!(
            impact.all_dependants,
            vec![FileId(1), FileId(3), FileId(2), FileId(4)]
        );
    }

    #[test]
    fn avoids_cycles_while_traversing_reverse_dependants() {
        let graph = IncludeGraph {
            files: vec![
                file(0, "/repo/include/a.h"),
                file(1, "/repo/include/b.h"),
                file(2, "/repo/src/main.cpp"),
            ],
            edges: vec![edge(0, 1, "b.h"), edge(1, 0, "a.h"), edge(2, 0, "a.h")],
        };

        let impact = analyze_include_impact(&graph, FileId(0));

        assert_eq!(impact.direct_dependants, vec![FileId(1), FileId(2)]);
        assert_eq!(impact.transitive_dependants, Vec::<FileId>::new());
        assert_eq!(impact.all_dependants, vec![FileId(1), FileId(2)]);
    }

    #[test]
    fn classifies_translation_units_headers_and_other_files() {
        assert_eq!(classify_file("main.c"), FileKind::TranslationUnit);
        assert_eq!(classify_file("main.cc"), FileKind::TranslationUnit);
        assert_eq!(classify_file("main.cpp"), FileKind::TranslationUnit);
        assert_eq!(classify_file("main.cxx"), FileKind::TranslationUnit);
        assert_eq!(classify_file("config.h"), FileKind::Header);
        assert_eq!(classify_file("config.hh"), FileKind::Header);
        assert_eq!(classify_file("config.hpp"), FileKind::Header);
        assert_eq!(classify_file("config.hxx"), FileKind::Header);
        assert_eq!(classify_file("config.inc"), FileKind::Other);
    }

    #[test]
    fn renders_impact_summary() {
        let graph = IncludeGraph {
            files: vec![
                file(0, "/repo/include/config.h"),
                file(1, "/repo/include/app.h"),
                file(2, "/repo/src/main.cpp"),
            ],
            edges: vec![edge(1, 0, "config.h"), edge(2, 1, "app.h")],
        };
        let impact = analyze_include_impact(&graph, FileId(0));

        assert_eq!(
            render_impact_result(&graph, &impact, "/repo"),
            concat!(
                "include/config.h impacts 2 files.\n",
                "\n",
                "Translation units:\n",
                "  src/main.cpp\n",
                "\n",
                "Headers:\n",
                "  include/app.h\n",
                "\n",
                "Direct dependants:\n",
                "  include/app.h\n",
                "\n",
                "Transitive dependants:\n",
                "  src/main.cpp\n",
            )
        );
    }

    fn file(id: usize, path: &str) -> FileNode {
        FileNode {
            id: FileId(id),
            path: PathBuf::from(path),
        }
    }

    fn edge(from: usize, to: usize, include_path: &str) -> IncludeEdge {
        IncludeEdge {
            from: FileId(from),
            to: IncludeTarget::Resolved(FileId(to)),
            include_path: include_path.to_owned(),
            kind: IncludeKind::Quote,
            line: 1,
        }
    }
}
