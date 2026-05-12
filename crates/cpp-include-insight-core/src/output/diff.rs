use crate::{
    IncludeGraphSnapshot, IncludeKind, SNAPSHOT_VERSION,
    output::snapshot::{SnapshotEdge, SnapshotStats, SnapshotTarget},
};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDiff {
    pub old_stats: SnapshotStats,
    pub new_stats: SnapshotStats,
    pub added_resolved: Vec<ResolvedDependencyChange>,
    pub removed_resolved: Vec<ResolvedDependencyChange>,
    pub newly_missing: Vec<MissingIncludeChange>,
    pub newly_resolved: Vec<ResolvedDependencyChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDependencyChange {
    pub from: String,
    pub to: String,
    pub include: String,
    pub kind: IncludeKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingIncludeChange {
    pub from: String,
    pub include: String,
    pub kind: IncludeKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ResolvedDependencyKey {
    from: String,
    to: String,
    include: String,
    kind: IncludeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IncludeRequestKey {
    from: String,
    include: String,
    kind: IncludeKind,
}

pub fn load_include_graph_snapshot(path: impl AsRef<Path>) -> Result<IncludeGraphSnapshot> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read snapshot {}", path.display()))?;
    let snapshot = serde_json::from_str::<IncludeGraphSnapshot>(&content)
        .with_context(|| format!("failed to parse snapshot {}", path.display()))?;

    validate_include_graph_snapshot(&snapshot, &path.display().to_string())?;

    Ok(snapshot)
}

pub fn validate_include_graph_snapshot(snapshot: &IncludeGraphSnapshot, label: &str) -> Result<()> {
    if snapshot.version != SNAPSHOT_VERSION {
        bail!(
            "{label} uses unsupported snapshot version {}; expected {}",
            snapshot.version,
            SNAPSHOT_VERSION
        );
    }

    if snapshot.root.path.is_empty() {
        bail!("{label} has an empty root path");
    }

    let file_paths = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<BTreeSet<_>>();

    if file_paths.len() != snapshot.files.len() {
        bail!("{label} contains duplicate file paths");
    }

    for edge in &snapshot.edges {
        if !file_paths.contains(edge.from.as_str()) {
            bail!(
                "{label} has an edge from unknown file {}",
                edge.from.as_str()
            );
        }

        if let SnapshotTarget::Resolved { path } = &edge.to
            && !file_paths.contains(path.as_str())
        {
            bail!("{label} has an edge to unknown file {}", path.as_str());
        }
    }

    let resolved = snapshot
        .edges
        .iter()
        .filter(|edge| matches!(edge.to, SnapshotTarget::Resolved { .. }))
        .count();
    let external = snapshot
        .edges
        .iter()
        .filter(|edge| matches!(edge.to, SnapshotTarget::External { .. }))
        .count();
    let missing = snapshot
        .edges
        .iter()
        .filter(|edge| matches!(edge.to, SnapshotTarget::Missing { .. }))
        .count();

    let expected_stats = SnapshotStats {
        files: snapshot.files.len(),
        edges: snapshot.edges.len(),
        resolved,
        external,
        missing,
        cycles: snapshot.cycles.len(),
    };

    if snapshot.stats != expected_stats {
        bail!(
            "{label} stats do not match snapshot contents: expected {:?}, found {:?}",
            expected_stats,
            snapshot.stats
        );
    }

    Ok(())
}

pub fn diff_include_graph_snapshots(
    old: &IncludeGraphSnapshot,
    new: &IncludeGraphSnapshot,
) -> Result<SnapshotDiff> {
    validate_include_graph_snapshot(old, "old snapshot")?;
    validate_include_graph_snapshot(new, "new snapshot")?;

    let old_resolved = resolved_edges_by_key(old);
    let new_resolved = resolved_edges_by_key(new);
    let old_missing = missing_edges_by_request(old);
    let new_missing = missing_edges_by_request(new);
    let new_resolved_by_request = resolved_edges_by_request(new);

    let added_resolved = new_resolved
        .iter()
        .filter(|(key, _)| !old_resolved.contains_key(*key))
        .map(|(_, change)| change.clone())
        .collect();
    let removed_resolved = old_resolved
        .iter()
        .filter(|(key, _)| !new_resolved.contains_key(*key))
        .map(|(_, change)| change.clone())
        .collect();
    let newly_missing = new_missing
        .iter()
        .filter(|(key, _)| !old_missing.contains_key(*key))
        .map(|(_, change)| change.clone())
        .collect();
    let newly_resolved = old_missing
        .keys()
        .filter_map(|key| new_resolved_by_request.get(key))
        .cloned()
        .collect();

    Ok(SnapshotDiff {
        old_stats: old.stats.clone(),
        new_stats: new.stats.clone(),
        added_resolved,
        removed_resolved,
        newly_missing,
        newly_resolved,
    })
}

pub fn render_snapshot_diff(diff: &SnapshotDiff) -> String {
    let mut output = String::new();

    output.push_str("Snapshot diff summary:\n");
    write_metric(
        &mut output,
        "Files",
        diff.old_stats.files,
        diff.new_stats.files,
    );
    write_metric(
        &mut output,
        "Edges",
        diff.old_stats.edges,
        diff.new_stats.edges,
    );
    write_metric(
        &mut output,
        "Resolved",
        diff.old_stats.resolved,
        diff.new_stats.resolved,
    );
    write_metric(
        &mut output,
        "External",
        diff.old_stats.external,
        diff.new_stats.external,
    );
    write_metric(
        &mut output,
        "Missing",
        diff.old_stats.missing,
        diff.new_stats.missing,
    );
    write_metric(
        &mut output,
        "Cycles",
        diff.old_stats.cycles,
        diff.new_stats.cycles,
    );

    write_resolved_section(
        &mut output,
        "Added resolved dependencies",
        &diff.added_resolved,
    );
    write_resolved_section(
        &mut output,
        "Removed resolved dependencies",
        &diff.removed_resolved,
    );
    write_missing_section(&mut output, "Newly missing includes", &diff.newly_missing);
    write_resolved_section(&mut output, "Newly resolved includes", &diff.newly_resolved);

    output
}

fn resolved_edges_by_key(
    snapshot: &IncludeGraphSnapshot,
) -> BTreeMap<ResolvedDependencyKey, ResolvedDependencyChange> {
    snapshot
        .edges
        .iter()
        .filter_map(|edge| {
            let SnapshotTarget::Resolved { path } = &edge.to else {
                return None;
            };
            let key = ResolvedDependencyKey {
                from: edge.from.clone(),
                to: path.clone(),
                include: edge.include.clone(),
                kind: edge.kind,
            };

            Some((
                key,
                ResolvedDependencyChange {
                    from: edge.from.clone(),
                    to: path.clone(),
                    include: edge.include.clone(),
                    kind: edge.kind,
                    line: edge.line,
                },
            ))
        })
        .collect()
}

fn resolved_edges_by_request(
    snapshot: &IncludeGraphSnapshot,
) -> BTreeMap<IncludeRequestKey, ResolvedDependencyChange> {
    snapshot
        .edges
        .iter()
        .filter_map(|edge| {
            let SnapshotTarget::Resolved { path } = &edge.to else {
                return None;
            };
            let key = include_request_key(edge);

            Some((
                key,
                ResolvedDependencyChange {
                    from: edge.from.clone(),
                    to: path.clone(),
                    include: edge.include.clone(),
                    kind: edge.kind,
                    line: edge.line,
                },
            ))
        })
        .collect()
}

fn missing_edges_by_request(
    snapshot: &IncludeGraphSnapshot,
) -> BTreeMap<IncludeRequestKey, MissingIncludeChange> {
    snapshot
        .edges
        .iter()
        .filter_map(|edge| {
            let SnapshotTarget::Missing { .. } = &edge.to else {
                return None;
            };
            let key = include_request_key(edge);

            Some((
                key,
                MissingIncludeChange {
                    from: edge.from.clone(),
                    include: edge.include.clone(),
                    kind: edge.kind,
                    line: edge.line,
                },
            ))
        })
        .collect()
}

fn include_request_key(edge: &SnapshotEdge) -> IncludeRequestKey {
    IncludeRequestKey {
        from: edge.from.clone(),
        include: edge.include.clone(),
        kind: edge.kind,
    }
}

fn write_metric(output: &mut String, label: &str, old: usize, new: usize) {
    let delta = new as isize - old as isize;
    let sign = if delta >= 0 { "+" } else { "" };

    let _ = writeln!(output, "{label}: {old} -> {new} ({sign}{delta})");
}

fn write_resolved_section(
    output: &mut String,
    heading: &str,
    changes: &[ResolvedDependencyChange],
) {
    let _ = writeln!(output, "\n{heading} ({}):", changes.len());

    if changes.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for change in changes {
        let _ = writeln!(
            output,
            "  {}:{} -> {} (include {})",
            change.from,
            change.line,
            change.to,
            format_include(change.kind, &change.include)
        );
    }
}

fn write_missing_section(output: &mut String, heading: &str, changes: &[MissingIncludeChange]) {
    let _ = writeln!(output, "\n{heading} ({}):", changes.len());

    if changes.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for change in changes {
        let _ = writeln!(
            output,
            "  {}:{} -> missing {} (include {})",
            change.from,
            change.line,
            change.include,
            format_include(change.kind, &change.include)
        );
    }
}

fn format_include(kind: IncludeKind, include: &str) -> String {
    match kind {
        IncludeKind::Quote => format!("\"{include}\""),
        IncludeKind::Angle => format!("<{include}>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::snapshot::{SnapshotFile, SnapshotPathStyle, SnapshotRoot, SnapshotTarget};

    #[test]
    fn reports_added_removed_missing_and_resolved_changes_in_stable_order() {
        let old = test_snapshot(vec![
            resolved_edge("src/main.cpp", "include/app.h", "app.h", 1),
            resolved_edge("src/main.cpp", "include/old.h", "old.h", 2),
            missing_edge("src/main.cpp", "generated.h", 3),
        ]);
        let new = test_snapshot(vec![
            resolved_edge("src/main.cpp", "include/app.h", "app.h", 1),
            resolved_edge("src/main.cpp", "include/generated.h", "generated.h", 3),
            resolved_edge("src/main.cpp", "include/new.h", "new.h", 4),
            missing_edge("include/app.h", "config.h", 2),
        ]);
        let diff = diff_include_graph_snapshots(&old, &new).unwrap();

        assert_eq!(diff.added_resolved.len(), 2);
        assert_eq!(diff.removed_resolved.len(), 1);
        assert_eq!(diff.newly_missing.len(), 1);
        assert_eq!(diff.newly_resolved.len(), 1);
        assert_eq!(diff.added_resolved[0].to, "include/generated.h");
        assert_eq!(diff.added_resolved[1].to, "include/new.h");
    }

    fn test_snapshot(edges: Vec<SnapshotEdge>) -> IncludeGraphSnapshot {
        let mut files = edges
            .iter()
            .flat_map(|edge| {
                let mut paths = vec![edge.from.clone()];
                if let SnapshotTarget::Resolved { path } = &edge.to {
                    paths.push(path.clone());
                }
                paths
            })
            .collect::<Vec<_>>();
        files.sort();
        files.dedup();

        let resolved = edges
            .iter()
            .filter(|edge| matches!(edge.to, SnapshotTarget::Resolved { .. }))
            .count();
        let external = edges
            .iter()
            .filter(|edge| matches!(edge.to, SnapshotTarget::External { .. }))
            .count();
        let missing = edges
            .iter()
            .filter(|edge| matches!(edge.to, SnapshotTarget::Missing { .. }))
            .count();

        IncludeGraphSnapshot {
            version: SNAPSHOT_VERSION,
            root: SnapshotRoot {
                path: ".".to_owned(),
                path_style: SnapshotPathStyle::ProjectRelative,
            },
            stats: SnapshotStats {
                files: files.len(),
                edges: edges.len(),
                resolved,
                external,
                missing,
                cycles: 0,
            },
            files: files
                .into_iter()
                .map(|path| SnapshotFile { path })
                .collect(),
            edges,
            cycles: vec![],
        }
    }

    fn resolved_edge(from: &str, to: &str, include: &str, line: usize) -> SnapshotEdge {
        SnapshotEdge {
            from: from.to_owned(),
            to: SnapshotTarget::Resolved {
                path: to.to_owned(),
            },
            include: include.to_owned(),
            kind: IncludeKind::Quote,
            line,
        }
    }

    fn missing_edge(from: &str, include: &str, line: usize) -> SnapshotEdge {
        SnapshotEdge {
            from: from.to_owned(),
            to: SnapshotTarget::Missing {
                include: include.to_owned(),
            },
            include: include.to_owned(),
            kind: IncludeKind::Quote,
            line,
        }
    }
}
