use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use cpp_include_insight_core::{ScanOptions, scan_project};
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
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
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
            let options = ScanOptions { include_dirs };
            let result = scan_project(path, &options)?;

            match format {
                OutputFormat::Text => {
                    println!("Scanned {} files.", result.files_scanned);
                    println!("Found {} include directives.", result.includes_found);

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
    }

    Ok(())
}
