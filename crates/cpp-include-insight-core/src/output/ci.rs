use crate::SnapshotDiff;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{fmt::Write as _, path::PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub include_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub ci: CiRules,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiRules {
    #[serde(default)]
    pub fail_on_new_cycle: bool,
    #[serde(default)]
    pub fail_on_missing_include: bool,
    #[serde(default)]
    pub max_impact_delta: Option<usize>,
    #[serde(default)]
    pub max_new_edges: Option<usize>,
    #[serde(default)]
    pub banned_includes: Vec<BannedIncludeRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BannedIncludeRule {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiCheckReport {
    pub checked_new_edges: usize,
    pub checked_new_missing: usize,
    pub checked_new_cycles: usize,
    pub checked_impact_deltas: usize,
    pub violations: Vec<CiViolation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiViolation {
    pub rule: String,
    pub message: String,
}

impl CiCheckReport {
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

pub fn evaluate_ci_rules(diff: &SnapshotDiff, config: &ProjectConfig) -> CiCheckReport {
    let mut violations = Vec::new();
    let rules = &config.ci;

    if rules.fail_on_new_cycle && !diff.new_cycles.is_empty() {
        violations.push(CiViolation {
            rule: "fail_on_new_cycle".to_owned(),
            message: format!("introduced {} new include cycle(s)", diff.new_cycles.len()),
        });
    }

    if rules.fail_on_missing_include && !diff.newly_missing.is_empty() {
        violations.push(CiViolation {
            rule: "fail_on_missing_include".to_owned(),
            message: format!(
                "introduced {} newly missing include(s)",
                diff.newly_missing.len()
            ),
        });
    }

    if let Some(limit) = rules.max_new_edges
        && diff.added_resolved.len() > limit
    {
        violations.push(CiViolation {
            rule: "max_new_edges".to_owned(),
            message: format!(
                "added {} resolved include edge(s), exceeding limit {}",
                diff.added_resolved.len(),
                limit
            ),
        });
    }

    if let Some(limit) = rules.max_impact_delta {
        let offenders = diff
            .impact_deltas
            .iter()
            .filter(|change| change.delta > limit as isize)
            .collect::<Vec<_>>();

        if !offenders.is_empty() {
            let details = offenders
                .iter()
                .map(|change| format!("{} ({:+})", change.header, change.delta))
                .collect::<Vec<_>>()
                .join(", ");
            violations.push(CiViolation {
                rule: "max_impact_delta".to_owned(),
                message: format!(
                    "{} header impact delta(s) exceeded limit {}: {}",
                    offenders.len(),
                    limit,
                    details
                ),
            });
        }
    }

    for change in &diff.added_resolved {
        for rule in &rules.banned_includes {
            if path_pattern_matches(&rule.from, &change.from)
                && path_pattern_matches(&rule.to, &change.to)
            {
                let mut message = format!(
                    "{}:{} added banned include edge to {}",
                    change.from, change.line, change.to
                );

                if let Some(reason) = &rule.reason
                    && !reason.is_empty()
                {
                    let _ = write!(message, " ({reason})");
                }

                violations.push(CiViolation {
                    rule: "banned_includes".to_owned(),
                    message,
                });
            }
        }
    }

    CiCheckReport {
        checked_new_edges: diff.added_resolved.len(),
        checked_new_missing: diff.newly_missing.len(),
        checked_new_cycles: diff.new_cycles.len(),
        checked_impact_deltas: diff.impact_deltas.len(),
        violations,
    }
}

pub fn render_ci_check_report(report: &CiCheckReport) -> String {
    let mut output = String::new();

    if report.passed() {
        output.push_str("CI include checks passed.\n");
    } else {
        let _ = writeln!(
            output,
            "CI include checks failed ({}):",
            report.violations.len()
        );
    }

    let _ = writeln!(
        output,
        "Checked {} new edge(s), {} newly missing include(s), {} new cycle(s), {} impact delta(s).",
        report.checked_new_edges,
        report.checked_new_missing,
        report.checked_new_cycles,
        report.checked_impact_deltas
    );

    for violation in &report.violations {
        let _ = writeln!(output, "- {}: {}", violation.rule, violation.message);
    }

    output
}

fn path_pattern_matches(pattern: &str, path: &str) -> bool {
    let regex = pattern_to_regex(pattern);
    regex.is_match(path)
}

fn pattern_to_regex(pattern: &str) -> Regex {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                regex.push_str(".*");
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            _ => regex.push_str(&regex::escape(&ch.to_string())),
        }
    }

    regex.push('$');
    Regex::new(&regex).expect("generated path pattern regex should be valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IncludeKind,
        output::diff::{
            ImpactDeltaChange, MissingIncludeChange, ResolvedDependencyChange, SnapshotDiff,
        },
        output::snapshot::SnapshotStats,
    };

    #[test]
    fn passes_when_enabled_thresholds_are_not_violated() {
        let diff = test_diff();
        let config = ProjectConfig {
            ci: CiRules {
                fail_on_new_cycle: true,
                fail_on_missing_include: true,
                max_impact_delta: Some(1),
                max_new_edges: Some(1),
                banned_includes: vec![BannedIncludeRule {
                    from: "include/public/**".to_owned(),
                    to: "src/private/**".to_owned(),
                    reason: None,
                }],
            },
            ..ProjectConfig::default()
        };

        let report = evaluate_ci_rules(&diff, &config);

        assert!(report.passed());
        assert_eq!(
            render_ci_check_report(&report),
            concat!(
                "CI include checks passed.\n",
                "Checked 1 new edge(s), 0 newly missing include(s), 0 new cycle(s), 1 impact delta(s).\n",
            )
        );
    }

    #[test]
    fn reports_all_enabled_rule_violations() {
        let mut diff = test_diff();
        diff.newly_missing.push(MissingIncludeChange {
            from: "src/main.cpp".to_owned(),
            include: "missing.h".to_owned(),
            kind: IncludeKind::Quote,
            line: 2,
        });
        diff.added_resolved.push(ResolvedDependencyChange {
            from: "include/public/api.h".to_owned(),
            to: "src/private/detail.h".to_owned(),
            include: "private/detail.h".to_owned(),
            kind: IncludeKind::Quote,
            line: 3,
        });
        diff.impact_deltas.push(ImpactDeltaChange {
            header: "include/config.h".to_owned(),
            old_translation_units: 1,
            new_translation_units: 3,
            delta: 2,
        });
        diff.new_cycles.push(crate::SnapshotCycleChange {
            files: vec!["include/a.h".to_owned(), "include/b.h".to_owned()],
            edges: vec![],
        });
        let config = ProjectConfig {
            ci: CiRules {
                fail_on_new_cycle: true,
                fail_on_missing_include: true,
                max_impact_delta: Some(1),
                max_new_edges: Some(1),
                banned_includes: vec![BannedIncludeRule {
                    from: "include/public/**".to_owned(),
                    to: "src/private/**".to_owned(),
                    reason: Some("public headers must not include private headers".to_owned()),
                }],
            },
            ..ProjectConfig::default()
        };

        let report = evaluate_ci_rules(&diff, &config);

        assert!(!report.passed());
        assert_eq!(report.violations.len(), 5);
        assert!(render_ci_check_report(&report).contains("fail_on_new_cycle"));
        assert!(render_ci_check_report(&report).contains("fail_on_missing_include"));
        assert!(render_ci_check_report(&report).contains("max_new_edges"));
        assert!(render_ci_check_report(&report).contains("max_impact_delta"));
        assert!(render_ci_check_report(&report).contains("banned_includes"));
    }

    #[test]
    fn path_patterns_support_single_and_recursive_wildcards() {
        assert!(path_pattern_matches("include/**", "include/public/api.h"));
        assert!(path_pattern_matches(
            "src/private/*.h",
            "src/private/detail.h"
        ));
        assert!(!path_pattern_matches(
            "src/private/*.h",
            "src/private/nested/detail.h"
        ));
    }

    fn test_diff() -> SnapshotDiff {
        SnapshotDiff {
            old_stats: stats(),
            new_stats: stats(),
            added_resolved: vec![ResolvedDependencyChange {
                from: "src/main.cpp".to_owned(),
                to: "include/app.h".to_owned(),
                include: "app.h".to_owned(),
                kind: IncludeKind::Quote,
                line: 1,
            }],
            removed_resolved: vec![],
            newly_missing: vec![],
            newly_resolved: vec![],
            new_cycles: vec![],
            impact_deltas: vec![ImpactDeltaChange {
                header: "include/app.h".to_owned(),
                old_translation_units: 0,
                new_translation_units: 1,
                delta: 1,
            }],
        }
    }

    fn stats() -> SnapshotStats {
        SnapshotStats {
            files: 0,
            edges: 0,
            resolved: 0,
            external: 0,
            missing: 0,
            cycles: 0,
        }
    }
}
