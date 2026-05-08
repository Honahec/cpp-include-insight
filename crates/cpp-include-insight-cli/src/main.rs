use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use cpp_include_insight_core::{
    IncludeGraph, IncludeResolver, ScanOptions, graph_to_json_value, scan_project,
};
use std::path::PathBuf;

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
        /// Project root
        path: PathBuf,

        /// Include directories.
        #[arg(short = 'I', long = "include-dir")]
        include_dirs: Vec<PathBuf>,

        /// Output format.
        #[arg(long, value_enum, default_value_t = GraphOutputFormat::Json)]
        format: GraphOutputFormat,
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
        } => {
            let options = ScanOptions {
                include_dirs: include_dirs.clone(),
            };
            let result = scan_project(&path, &options)?;
            let resolver = IncludeResolver::new(&path, include_dirs);
            let graph = IncludeGraph::from_scan_result(&result, &resolver);

            match format {
                GraphOutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&graph_to_json_value(&graph))?
                    );
                }
            }
        }
    }

    Ok(())
}
