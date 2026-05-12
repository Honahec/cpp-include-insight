use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use cpp_include_insight_core::{
    DEFAULT_MAX_WHY_PATHS, IncludeGraph, IncludeResolver, MermaidOptions, ScanOptions,
    SnapshotOptions, WhyOptions, analyze_include_impact, build_include_graph_snapshot,
    detect_include_cycles, diff_include_graph_snapshots, find_include_paths, graph_to_json_value,
    load_include_graph_snapshot, render_impact_result, render_include_cycles, render_include_tree,
    render_mermaid_graph, render_reverse_include_tree, render_snapshot_diff, render_why_result,
    scan_project,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(name = "cpp-include-insight")]
#[command(about = "C/C++ include impact analyzer")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Scan C/C++ files and print include directives.
    Scan {
        /// Project root
        path: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,

        /// Output format.
        #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },

    /// Build the include dependency graph.
    Graph {
        /// Project root or root file for graph slices.
        path: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,

        /// Output format.
        #[arg(long, value_enum, default_value_t = GraphOutputFormat::Json)]
        format: GraphOutputFormat,

        /// Maximum include depth when rendering from a root file.
        #[arg(long)]
        depth: Option<usize>,

        /// Omit external angle includes from Mermaid output.
        #[arg(long = "no-external")]
        no_external: bool,
    },

    /// Print the forward include tree for a source file.
    Tree {
        /// Root source file
        file: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,
    },

    /// Print the reverse include tree for a source or header file.
    Rtree {
        /// Target source or header file
        file: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,
    },

    /// Detect include cycles in the resolved project graph.
    Cycles {
        /// Project root
        path: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,
    },

    /// Explain why one project file depends on another.
    Why {
        /// Source file
        source: PathBuf,

        /// Target file
        target: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,

        /// Maximum number of paths to print.
        #[arg(long = "max-paths", conflicts_with = "all", value_parser = parse_positive_usize)]
        max_paths: Option<usize>,

        /// Print only the shortest dependency path.
        #[arg(long, conflicts_with = "all")]
        shortest: bool,

        /// Search exhaustively for all simple dependency paths.
        #[arg(long)]
        all: bool,
    },

    /// Report files affected by changes to a source or header file.
    Impact {
        /// Target source or header file
        file: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,
    },

    /// Write a stable, versioned include graph snapshot.
    Snapshot {
        /// Project root
        path: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,

        /// Snapshot output file.
        #[arg(short = 'o', long = "output")]
        output: PathBuf,

        /// Store absolute paths instead of project-relative paths.
        #[arg(long = "absolute-paths")]
        absolute_paths: bool,
    },

    /// Compare two include graph snapshots.
    Diff {
        /// Older snapshot JSON file.
        old: PathBuf,

        /// Newer snapshot JSON file.
        new: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum GraphOutputFormat {
    Json,
    Mermaid,
}

fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("expected a positive integer: {error}"))?;

    if parsed == 0 {
        return Err("value must be greater than 0".to_owned());
    }

    Ok(parsed)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Scan {
            path,
            include_dirs,
            format,
        } => {
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&path, &options)?;
            let resolver = IncludeResolver::new(&path, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);

            match format {
                OutputFormat::Text => {
                    let stats = graph.stats();

                    println!("Scanned {} files.", result.files_scanned);
                    println!("Found {} include directives.", result.includes_found);
                    println!("Resolved {} project includes.", stats.resolved);
                    println!("External includes: {}", stats.external);
                    println!("Missing includes: {}", stats.missing);

                    for file in result.files {
                        for include in file.includes {
                            println!(
                                "{}:{} -> {}",
                                file.file.display(),
                                include.line,
                                include.path
                            )
                        }
                    }
                }

                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
            }
        }

        Command::Graph {
            path,
            include_dirs,
            format,
            depth,
            no_external,
        } => {
            let (project_root, root_file) = graph_input_context(&path)?;
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&project_root, &options)?;
            let resolver = IncludeResolver::new(&project_root, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);

            match format {
                GraphOutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&graph_to_json_value(&graph))?
                    );
                }
                GraphOutputFormat::Mermaid => {
                    let root = if let Some(root_file) = root_file {
                        Some(graph.file_id_for_path(&root_file).ok_or_else(|| {
                            anyhow::anyhow!(
                                "{} is not a scanned C/C++ source or header under {}",
                                root_file.display(),
                                project_root.display()
                            )
                        })?)
                    } else {
                        None
                    };

                    print!(
                        "{}",
                        render_mermaid_graph(
                            &graph,
                            &project_root,
                            MermaidOptions {
                                root,
                                max_depth: depth,
                                include_external: !no_external,
                            },
                        )
                    );
                }
            }
        }

        Command::Tree { file, include_dirs } => {
            let project_root = std::env::current_dir()?;
            let root_file = if file.is_absolute() {
                file
            } else {
                project_root.join(file)
            };
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&project_root, &options)?;
            let resolver = IncludeResolver::new(&project_root, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);
            let Some(root_id) = graph.file_id_for_path(&root_file) else {
                anyhow::bail!(
                    "{} is not a scanned C/C++ source or header under {}",
                    root_file.display(),
                    project_root.display()
                );
            };

            print!("{}", render_include_tree(&graph, root_id, &project_root));
        }

        Command::Rtree { file, include_dirs } => {
            let project_root = std::env::current_dir()?;
            let root_file = if file.is_absolute() {
                file
            } else {
                project_root.join(file)
            };
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&project_root, &options)?;
            let resolver = IncludeResolver::new(&project_root, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);
            let Some(root_id) = graph.file_id_for_path(&root_file) else {
                anyhow::bail!(
                    "{} is not a scanned C/C++ source or header under {}",
                    root_file.display(),
                    project_root.display()
                );
            };

            print!(
                "{}",
                render_reverse_include_tree(&graph, root_id, &project_root)
            );
        }

        Command::Cycles { path, include_dirs } => {
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&path, &options)?;
            let resolver = IncludeResolver::new(&path, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);
            let cycles = detect_include_cycles(&graph);

            print!("{}", render_include_cycles(&graph, &cycles, &path));
        }

        Command::Why {
            source,
            target,
            include_dirs,
            max_paths,
            shortest,
            all,
        } => {
            let project_root = std::env::current_dir()?;
            let source_file = if source.is_absolute() {
                source
            } else {
                project_root.join(source)
            };
            let target_file = if target.is_absolute() {
                target
            } else {
                project_root.join(target)
            };
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&project_root, &options)?;
            let resolver = IncludeResolver::new(&project_root, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);
            let Some(source_id) = graph.file_id_for_path(&source_file) else {
                anyhow::bail!(
                    "{} is not a scanned C/C++ source or header under {}",
                    source_file.display(),
                    project_root.display()
                );
            };
            let Some(target_id) = graph.file_id_for_path(&target_file) else {
                anyhow::bail!(
                    "{} is not a scanned C/C++ source or header under {}",
                    target_file.display(),
                    project_root.display()
                );
            };
            let why_options = WhyOptions {
                max_paths: if all {
                    None
                } else {
                    max_paths.or(Some(DEFAULT_MAX_WHY_PATHS))
                },
                shortest,
            };
            let why = find_include_paths(&graph, source_id, target_id, &why_options);

            print!("{}", render_why_result(&graph, &why, &project_root));
        }

        Command::Impact { file, include_dirs } => {
            let project_root = std::env::current_dir()?;
            let target_file = if file.is_absolute() {
                file
            } else {
                project_root.join(file)
            };
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&project_root, &options)?;
            let resolver = IncludeResolver::new(&project_root, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);
            let Some(target_id) = graph.file_id_for_path(&target_file) else {
                anyhow::bail!(
                    "{} is not a scanned C/C++ source or header under {}",
                    target_file.display(),
                    project_root.display()
                );
            };
            let impact = analyze_include_impact(&graph, target_id);

            print!("{}", render_impact_result(&graph, &impact, &project_root));
        }

        Command::Snapshot {
            path,
            include_dirs,
            output,
            absolute_paths,
        } => {
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&path, &options)?;
            let resolver = IncludeResolver::new(&path, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);
            let cycles = detect_include_cycles(&graph);
            let snapshot = build_include_graph_snapshot(
                &graph,
                &cycles,
                &path,
                SnapshotOptions { absolute_paths },
            );
            let json = serde_json::to_string_pretty(&snapshot)?;

            if let Some(parent) = output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }

            fs::write(output, format!("{json}\n"))?;
        }

        Command::Diff { old, new } => {
            let old = load_include_graph_snapshot(old)?;
            let new = load_include_graph_snapshot(new)?;
            let diff = diff_include_graph_snapshots(&old, &new)?;

            print!("{}", render_snapshot_diff(&diff));
        }
    }

    Ok(())
}

fn graph_input_context(path: &Path) -> Result<(PathBuf, Option<PathBuf>)> {
    let current_dir = std::env::current_dir()?;
    let input = if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_dir.join(path)
    };

    if input.is_file() {
        Ok((current_dir, Some(input)))
    } else {
        Ok((path.to_path_buf(), None))
    }
}
