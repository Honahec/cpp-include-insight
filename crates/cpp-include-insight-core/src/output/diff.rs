use crate::{
    IncludeGraphSnapshot, IncludeKind, SNAPSHOT_VERSION,
    analysis::impact::{FileKind, classify_file},
    output::snapshot::{
        SnapshotCycle, SnapshotCycleEdge, SnapshotEdge, SnapshotStats, SnapshotTarget,
    },
};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt::Write as _,
    fs,
    path::Path,
};

const DEFAULT_MAX_IMPACT_DELTA_CHANGES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotDiff {
    pub old_stats: SnapshotStats,
    pub new_stats: SnapshotStats,
    pub added_resolved: Vec<ResolvedDependencyChange>,
    pub removed_resolved: Vec<ResolvedDependencyChange>,
    pub newly_missing: Vec<MissingIncludeChange>,
    pub newly_resolved: Vec<ResolvedDependencyChange>,
    pub new_cycles: Vec<SnapshotCycleChange>,
    pub impact_deltas: Vec<ImpactDeltaChange>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotCycleChange {
    pub files: Vec<String>,
    pub edges: Vec<SnapshotCycleEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactDeltaChange {
    pub header: String,
    pub old_translation_units: usize,
    pub new_translation_units: usize,
    pub delta: isize,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CycleKey {
    files: Vec<String>,
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

    for cycle in &snapshot.cycles {
        for file in &cycle.files {
            if !file_paths.contains(file.as_str()) {
                bail!("{label} has a cycle containing unknown file {}", file);
            }
        }

        for edge in &cycle.edges {
            if !file_paths.contains(edge.from.as_str()) {
                bail!("{label} has a cycle edge from unknown file {}", edge.from);
            }

            if !file_paths.contains(edge.to.as_str()) {
                bail!("{label} has a cycle edge to unknown file {}", edge.to);
            }
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
    let old_cycles = cycles_by_key(old);
    let new_cycles_by_key = cycles_by_key(new);

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
    let new_cycles = new_cycles_by_key
        .iter()
        .filter(|(key, _)| !old_cycles.contains_key(*key))
        .map(|(_, cycle)| cycle.clone())
        .collect();

    Ok(SnapshotDiff {
        old_stats: old.stats.clone(),
        new_stats: new.stats.clone(),
        added_resolved,
        removed_resolved,
        newly_missing,
        newly_resolved,
        new_cycles,
        impact_deltas: impact_delta_changes(old, new),
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
    write_cycle_section(&mut output, "New include cycles", &diff.new_cycles);

    output
}

pub fn render_snapshot_diff_with_impact(diff: &SnapshotDiff) -> String {
    let mut output = render_snapshot_diff(diff);
    write_impact_delta_sections(&mut output, &diff.impact_deltas);
    output
}

fn impact_delta_changes(
    old: &IncludeGraphSnapshot,
    new: &IncludeGraphSnapshot,
) -> Vec<ImpactDeltaChange> {
    let old_counts = snapshot_header_impact_counts(old);
    let new_counts = snapshot_header_impact_counts(new);
    let headers = old_counts
        .keys()
        .chain(new_counts.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    headers
        .into_iter()
        .filter_map(|header| {
            let old_translation_units = old_counts.get(&header).copied().unwrap_or(0);
            let new_translation_units = new_counts.get(&header).copied().unwrap_or(0);
            let delta = new_translation_units as isize - old_translation_units as isize;

            (delta != 0).then_some(ImpactDeltaChange {
                header,
                old_translation_units,
                new_translation_units,
                delta,
            })
        })
        .collect()
}

fn snapshot_header_impact_counts(snapshot: &IncludeGraphSnapshot) -> BTreeMap<String, usize> {
    let headers = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .filter(|path| classify_file(path) == FileKind::Header)
        .collect::<BTreeSet<_>>();
    let translation_units = snapshot
        .files
        .iter()
        .map(|file| file.path.as_str())
        .filter(|path| classify_file(path) == FileKind::TranslationUnit)
        .collect::<BTreeSet<_>>();
    let mut reverse_edges = BTreeMap::<&str, BTreeSet<&str>>::new();

    for edge in &snapshot.edges {
        if let SnapshotTarget::Resolved { path } = &edge.to {
            reverse_edges
                .entry(path.as_str())
                .or_default()
                .insert(edge.from.as_str());
        }
    }

    headers
        .iter()
        .map(|header| {
            let mut seen = BTreeSet::from([*header]);
            let mut queue = VecDeque::from([*header]);
            let mut impacted_translation_units = BTreeSet::new();

            while let Some(file) = queue.pop_front() {
                for dependant in reverse_edges.get(file).into_iter().flatten() {
                    let dependant = *dependant;

                    if !seen.insert(dependant) {
                        continue;
                    }

                    if translation_units.contains(dependant) {
                        impacted_translation_units.insert(dependant);
                    }

                    queue.push_back(dependant);
                }
            }

            ((*header).to_owned(), impacted_translation_units.len())
        })
        .collect()
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

fn cycles_by_key(snapshot: &IncludeGraphSnapshot) -> BTreeMap<CycleKey, SnapshotCycleChange> {
    snapshot
        .cycles
        .iter()
        .map(|cycle| {
            (
                cycle_key(cycle),
                SnapshotCycleChange {
                    files: cycle.files.clone(),
                    edges: cycle.edges.clone(),
                },
            )
        })
        .collect()
}

fn cycle_key(cycle: &SnapshotCycle) -> CycleKey {
    let mut files = cycle.files.clone();
    files.sort();

    CycleKey { files }
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

fn write_cycle_section(output: &mut String, heading: &str, cycles: &[SnapshotCycleChange]) {
    let _ = writeln!(output, "\n{heading} ({}):", cycles.len());

    if cycles.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for (index, cycle) in cycles.iter().enumerate() {
        let _ = writeln!(output, "  Cycle {}:", index + 1);
        write_cycle_path(output, cycle);
    }
}

fn write_cycle_path(output: &mut String, cycle: &SnapshotCycleChange) {
    let Some(path) = find_cycle_path(cycle) else {
        for edge in &cycle.edges {
            let _ = writeln!(output, "    {}:{} -> {}", edge.from, edge.line, edge.to);
        }
        return;
    };

    for (index, edge) in path.iter().enumerate() {
        if index == 0 {
            let _ = writeln!(output, "    {}:{}", edge.from, edge.line);
        }

        output.push_str("      -> ");
        output.push_str(&edge.to);

        if let Some(next_edge) = path.get(index + 1) {
            let _ = write!(output, ":{}", next_edge.line);
        }

        output.push('\n');
    }
}

fn find_cycle_path(cycle: &SnapshotCycleChange) -> Option<Vec<&SnapshotCycleEdge>> {
    let start = cycle.files.first()?;
    let mut stack = BTreeSet::from([start.as_str()]);
    let mut path = Vec::new();

    if find_cycle_path_from(cycle, start, start, &mut stack, &mut path) {
        Some(path)
    } else {
        None
    }
}

fn find_cycle_path_from<'a>(
    cycle: &'a SnapshotCycleChange,
    start: &str,
    current: &str,
    stack: &mut BTreeSet<&'a str>,
    path: &mut Vec<&'a SnapshotCycleEdge>,
) -> bool {
    for edge in cycle.edges.iter().filter(|edge| edge.from == current) {
        path.push(edge);

        if edge.to == start {
            return true;
        }

        if !stack.contains(edge.to.as_str()) {
            stack.insert(edge.to.as_str());

            if find_cycle_path_from(cycle, start, &edge.to, stack, path) {
                return true;
            }

            stack.remove(edge.to.as_str());
        }

        path.pop();
    }

    false
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

fn write_impact_delta_sections(output: &mut String, changes: &[ImpactDeltaChange]) {
    let increases = changes
        .iter()
        .filter(|change| change.delta > 0)
        .collect::<Vec<_>>();
    let decreases = changes
        .iter()
        .filter(|change| change.delta < 0)
        .collect::<Vec<_>>();

    write_impact_delta_section(output, "Impact increases", increases, true);
    write_impact_delta_section(output, "Impact decreases", decreases, false);
}

fn write_impact_delta_section(
    output: &mut String,
    heading: &str,
    mut changes: Vec<&ImpactDeltaChange>,
    largest_delta_first: bool,
) {
    changes.sort_by(|left, right| {
        let delta_order = if largest_delta_first {
            right.delta.cmp(&left.delta)
        } else {
            left.delta.cmp(&right.delta)
        };

        delta_order.then_with(|| left.header.cmp(&right.header))
    });

    let _ = writeln!(output, "\n{heading} ({}):", changes.len());

    if changes.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for change in changes.iter().take(DEFAULT_MAX_IMPACT_DELTA_CHANGES) {
        let _ = writeln!(
            output,
            "  {}: {} -> {} ({:+} translation units)",
            change.header, change.old_translation_units, change.new_translation_units, change.delta
        );
    }

    if changes.len() > DEFAULT_MAX_IMPACT_DELTA_CHANGES {
        let _ = writeln!(
            output,
            "  ... {} more not shown",
            changes.len() - DEFAULT_MAX_IMPACT_DELTA_CHANGES
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
        assert_eq!(diff.new_cycles.len(), 0);
        assert_eq!(diff.added_resolved[0].to, "include/generated.h");
        assert_eq!(diff.added_resolved[1].to, "include/new.h");
    }

    #[test]
    fn reports_only_cycles_absent_from_old_snapshot() {
        let old = test_snapshot_with_cycles(
            vec![
                resolved_edge("include/a.h", "include/b.h", "b.h", 1),
                resolved_edge("include/b.h", "include/a.h", "a.h", 2),
                resolved_edge("include/removed.h", "include/removed2.h", "removed2.h", 3),
                resolved_edge("include/removed2.h", "include/removed.h", "removed.h", 4),
            ],
            vec![
                cycle(vec![
                    cycle_edge("include/a.h", "include/b.h", "b.h", 1),
                    cycle_edge("include/b.h", "include/a.h", "a.h", 2),
                ]),
                cycle(vec![
                    cycle_edge("include/removed.h", "include/removed2.h", "removed2.h", 3),
                    cycle_edge("include/removed2.h", "include/removed.h", "removed.h", 4),
                ]),
            ],
        );
        let new = test_snapshot_with_cycles(
            vec![
                resolved_edge("include/a.h", "include/b.h", "b.h", 10),
                resolved_edge("include/b.h", "include/a.h", "a.h", 11),
                resolved_edge("include/c.h", "include/d.h", "d.h", 5),
                resolved_edge("include/d.h", "include/c.h", "c.h", 6),
            ],
            vec![
                cycle(vec![
                    cycle_edge("include/a.h", "include/b.h", "b.h", 10),
                    cycle_edge("include/b.h", "include/a.h", "a.h", 11),
                ]),
                cycle(vec![
                    cycle_edge("include/c.h", "include/d.h", "d.h", 5),
                    cycle_edge("include/d.h", "include/c.h", "c.h", 6),
                ]),
            ],
        );

        let diff = diff_include_graph_snapshots(&old, &new).unwrap();

        assert_eq!(diff.new_cycles.len(), 1);
        assert_eq!(diff.new_cycles[0].files, vec!["include/c.h", "include/d.h"]);
        assert_eq!(
            render_snapshot_diff(&diff),
            concat!(
                "Snapshot diff summary:\n",
                "Files: 4 -> 4 (+0)\n",
                "Edges: 4 -> 4 (+0)\n",
                "Resolved: 4 -> 4 (+0)\n",
                "External: 0 -> 0 (+0)\n",
                "Missing: 0 -> 0 (+0)\n",
                "Cycles: 2 -> 2 (+0)\n",
                "\n",
                "Added resolved dependencies (2):\n",
                "  include/c.h:5 -> include/d.h (include \"d.h\")\n",
                "  include/d.h:6 -> include/c.h (include \"c.h\")\n",
                "\n",
                "Removed resolved dependencies (2):\n",
                "  include/removed.h:3 -> include/removed2.h (include \"removed2.h\")\n",
                "  include/removed2.h:4 -> include/removed.h (include \"removed.h\")\n",
                "\n",
                "Newly missing includes (0):\n",
                "  (none)\n",
                "\n",
                "Newly resolved includes (0):\n",
                "  (none)\n",
                "\n",
                "New include cycles (1):\n",
                "  Cycle 1:\n",
                "    include/c.h:5\n",
                "      -> include/d.h:6\n",
                "      -> include/c.h\n",
            )
        );
    }

    #[test]
    fn reports_translation_unit_impact_deltas_for_headers() {
        let old = test_snapshot(vec![
            resolved_edge("include/app.h", "include/config.h", "config.h", 1),
            resolved_edge("src/main.cpp", "include/app.h", "app.h", 1),
            resolved_edge("src/old.cpp", "include/legacy.h", "legacy.h", 1),
            resolved_edge("src/stable.cpp", "include/stable.h", "stable.h", 1),
            resolved_edge("src/worker.cc", "include/config.h", "config.h", 1),
        ]);
        let new = test_snapshot(vec![
            resolved_edge("include/app.h", "include/config.h", "config.h", 1),
            resolved_edge("src/main.cpp", "include/app.h", "app.h", 1),
            resolved_edge("src/server.cxx", "include/config.h", "config.h", 1),
            resolved_edge("src/stable.cpp", "include/stable.h", "stable.h", 1),
            resolved_edge("src/worker.cc", "include/config.h", "config.h", 1),
        ]);
        let diff = diff_include_graph_snapshots(&old, &new).unwrap();

        assert_eq!(
            diff.impact_deltas,
            vec![
                ImpactDeltaChange {
                    header: "include/config.h".to_owned(),
                    old_translation_units: 2,
                    new_translation_units: 3,
                    delta: 1,
                },
                ImpactDeltaChange {
                    header: "include/legacy.h".to_owned(),
                    old_translation_units: 1,
                    new_translation_units: 0,
                    delta: -1,
                },
            ]
        );

        let rendered = render_snapshot_diff_with_impact(&diff);
        assert!(rendered.contains(
            "Impact increases (1):\n  include/config.h: 2 -> 3 (+1 translation units)\n"
        ));
        assert!(rendered.contains(
            "Impact decreases (1):\n  include/legacy.h: 1 -> 0 (-1 translation units)\n"
        ));
        assert!(!rendered.contains("include/stable.h:"));
    }

    fn test_snapshot(edges: Vec<SnapshotEdge>) -> IncludeGraphSnapshot {
        test_snapshot_with_cycles(edges, vec![])
    }

    fn test_snapshot_with_cycles(
        edges: Vec<SnapshotEdge>,
        cycles: Vec<SnapshotCycle>,
    ) -> IncludeGraphSnapshot {
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
        let cycle_count = cycles.len();

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
                cycles: cycle_count,
            },
            files: files
                .into_iter()
                .map(|path| SnapshotFile { path })
                .collect(),
            edges,
            cycles,
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

    fn cycle(edges: Vec<SnapshotCycleEdge>) -> SnapshotCycle {
        let mut files = edges
            .iter()
            .flat_map(|edge| [edge.from.clone(), edge.to.clone()])
            .collect::<Vec<_>>();
        files.sort();
        files.dedup();

        SnapshotCycle { files, edges }
    }

    fn cycle_edge(from: &str, to: &str, include: &str, line: usize) -> SnapshotCycleEdge {
        SnapshotCycleEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            include: include.to_owned(),
            line,
        }
    }
}
