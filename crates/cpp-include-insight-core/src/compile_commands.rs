use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSearchPaths {
    pub quote_dirs: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub system_dirs: Vec<PathBuf>,
}

impl FileSearchPaths {
    pub fn extend_include_dirs(&mut self, include_dirs: impl IntoIterator<Item = PathBuf>) {
        self.include_dirs.extend(include_dirs);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileCommand {
    pub directory: PathBuf,
    pub file: PathBuf,
    pub output: Option<String>,
    pub raw_command: Option<String>,
    pub arguments: Vec<String>,
    pub search_paths: FileSearchPaths,
    pub defines: Vec<String>,
    pub undefines: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompilationDatabase {
    pub commands: Vec<CompileCommand>,
}

#[derive(Debug, Deserialize)]
struct RawCompileCommand {
    directory: Option<PathBuf>,
    file: Option<PathBuf>,
    output: Option<String>,
    command: Option<String>,
    arguments: Option<Vec<String>>,
}

pub fn load_compilation_database(path: impl AsRef<Path>) -> Result<CompilationDatabase> {
    let path = path.as_ref();
    let database_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read compilation database {}", path.display()))?;
    let raw_entries = serde_json::from_str::<Vec<RawCompileCommand>>(&content)
        .with_context(|| format!("failed to parse compilation database {}", path.display()))?;

    let mut commands = Vec::with_capacity(raw_entries.len());

    for (index, entry) in raw_entries.into_iter().enumerate() {
        commands.push(parse_compile_command(index, entry, database_dir)?);
    }

    Ok(CompilationDatabase { commands })
}

impl CompilationDatabase {
    pub fn file_search_paths(&self) -> HashMap<PathBuf, FileSearchPaths> {
        self.commands
            .iter()
            .map(|command| (normalize_path(&command.file), command.search_paths.clone()))
            .collect()
    }

    pub fn rebase_paths(&self, from_root: &Path, to_root: &Path) -> Self {
        Self {
            commands: self
                .commands
                .iter()
                .map(|command| CompileCommand {
                    directory: rebase_path(&command.directory, from_root, to_root),
                    file: rebase_path(&command.file, from_root, to_root),
                    output: command.output.clone(),
                    raw_command: command.raw_command.clone(),
                    arguments: command.arguments.clone(),
                    search_paths: FileSearchPaths {
                        quote_dirs: command
                            .search_paths
                            .quote_dirs
                            .iter()
                            .map(|path| rebase_path(path, from_root, to_root))
                            .collect(),
                        include_dirs: command
                            .search_paths
                            .include_dirs
                            .iter()
                            .map(|path| rebase_path(path, from_root, to_root))
                            .collect(),
                        system_dirs: command
                            .search_paths
                            .system_dirs
                            .iter()
                            .map(|path| rebase_path(path, from_root, to_root))
                            .collect(),
                    },
                    defines: command.defines.clone(),
                    undefines: command.undefines.clone(),
                })
                .collect(),
        }
    }
}

fn parse_compile_command(
    index: usize,
    entry: RawCompileCommand,
    database_dir: &Path,
) -> Result<CompileCommand> {
    let directory = entry
        .directory
        .ok_or_else(|| anyhow!("compile_commands.json entry {index} is missing `directory`"))?;
    let directory = if directory.is_absolute() {
        directory
    } else {
        database_dir.join(directory)
    };
    let file = entry
        .file
        .ok_or_else(|| anyhow!("compile_commands.json entry {index} is missing `file`"))?;
    let file = if file.is_absolute() {
        file
    } else {
        directory.join(file)
    };

    let (arguments, raw_command) = match (entry.arguments, entry.command) {
        (Some(arguments), command) => (arguments, command),
        (None, Some(command)) => (split_shell_words(&command, index)?, Some(command)),
        (None, None) => {
            bail!("compile_commands.json entry {index} must contain `arguments` or `command`")
        }
    };

    let (search_paths, defines, undefines) = extract_compile_metadata(&directory, &arguments)?;

    Ok(CompileCommand {
        directory,
        file,
        output: entry.output,
        raw_command,
        arguments,
        search_paths,
        defines,
        undefines,
    })
}

fn extract_compile_metadata(
    directory: &Path,
    arguments: &[String],
) -> Result<(FileSearchPaths, Vec<String>, Vec<String>)> {
    let mut search_paths = FileSearchPaths::default();
    let mut defines = Vec::new();
    let mut undefines = Vec::new();
    let mut index = 0;

    while index < arguments.len() {
        let argument = &arguments[index];

        match argument.as_str() {
            "-I" => {
                let value = next_argument(arguments, index, "-I")?;
                search_paths
                    .include_dirs
                    .push(resolve_from(directory, value));
                index += 2;
            }
            "-iquote" => {
                let value = next_argument(arguments, index, "-iquote")?;
                search_paths.quote_dirs.push(resolve_from(directory, value));
                index += 2;
            }
            "-isystem" => {
                let value = next_argument(arguments, index, "-isystem")?;
                search_paths
                    .system_dirs
                    .push(resolve_from(directory, value));
                index += 2;
            }
            "-D" => {
                defines.push(next_argument(arguments, index, "-D")?.to_owned());
                index += 2;
            }
            "-U" => {
                undefines.push(next_argument(arguments, index, "-U")?.to_owned());
                index += 2;
            }
            _ if argument.starts_with("-I") && argument.len() > 2 => {
                search_paths
                    .include_dirs
                    .push(resolve_from(directory, &argument[2..]));
                index += 1;
            }
            _ if argument.starts_with("-iquote") && argument.len() > "-iquote".len() => {
                search_paths
                    .quote_dirs
                    .push(resolve_from(directory, &argument["-iquote".len()..]));
                index += 1;
            }
            _ if argument.starts_with("-isystem") && argument.len() > "-isystem".len() => {
                search_paths
                    .system_dirs
                    .push(resolve_from(directory, &argument["-isystem".len()..]));
                index += 1;
            }
            _ if argument.starts_with("-D") && argument.len() > 2 => {
                defines.push(argument[2..].to_owned());
                index += 1;
            }
            _ if argument.starts_with("-U") && argument.len() > 2 => {
                undefines.push(argument[2..].to_owned());
                index += 1;
            }
            _ => {
                index += 1;
            }
        }
    }

    Ok((search_paths, defines, undefines))
}

fn next_argument<'a>(arguments: &'a [String], index: usize, flag: &str) -> Result<&'a str> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("compile command flag `{flag}` is missing its value"))
}

fn resolve_from(directory: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);

    if path.is_absolute() {
        path
    } else {
        directory.join(path)
    }
}

fn split_shell_words(command: &str, index: usize) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote = None;

    while let Some(ch) = chars.next() {
        match (ch, quote) {
            ('\\', None) | ('\\', Some('"')) => {
                let Some(next) = chars.next() else {
                    current.push('\\');
                    break;
                };
                current.push(next);
            }
            ('\'', None) => quote = Some('\''),
            ('\'', Some('\'')) => quote = None,
            ('"', None) => quote = Some('"'),
            ('"', Some('"')) => quote = None,
            (ch, None) if ch.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            (ch, _) => current.push(ch),
        }
    }

    if let Some(quote) = quote {
        bail!(
            "compile_commands.json entry {index} has an unterminated `{quote}` quote in `command`"
        );
    }

    if !current.is_empty() {
        words.push(current);
    }

    Ok(words)
}

fn rebase_path(path: &Path, from_root: &Path, to_root: &Path) -> PathBuf {
    let normalized_path = normalize_path(path);
    let normalized_from = normalize_path(from_root);

    normalized_path
        .strip_prefix(&normalized_from)
        .map(|relative| to_root.join(relative))
        .unwrap_or(normalized_path)
}

fn normalize_path(path: &Path) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_arguments_form_and_relative_paths() {
        let temp = tempdir().unwrap();
        let database = temp.path().join("compile_commands.json");
        let directory = temp.path().join("build");
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            &database,
            format!(
                r#"[
  {{
    "directory": "{}",
    "file": "../src/main.cpp",
    "arguments": ["c++", "-I../include", "-iquote", "../quoted", "-isystem", "/opt/sdk/include", "-DDEBUG=1", "-UOLD"]
  }}
]"#,
                directory.display()
            ),
        )
        .unwrap();

        let database = load_compilation_database(&database).unwrap();
        let command = &database.commands[0];

        assert_eq!(command.file, directory.join("../src/main.cpp"));
        assert_eq!(
            command.search_paths.include_dirs,
            vec![directory.join("../include")]
        );
        assert_eq!(
            command.search_paths.quote_dirs,
            vec![directory.join("../quoted")]
        );
        assert_eq!(
            command.search_paths.system_dirs,
            vec![PathBuf::from("/opt/sdk/include")]
        );
        assert_eq!(command.defines, vec!["DEBUG=1"]);
        assert_eq!(command.undefines, vec!["OLD"]);
    }

    #[test]
    fn parses_command_form_with_quotes() {
        let temp = tempdir().unwrap();
        let database = temp.path().join("compile_commands.json");
        fs::write(
            &database,
            format!(
                r#"[
  {{
    "directory": "{}",
    "file": "src/main.cpp",
    "command": "c++ -I'include dir' -DMODE=\"fast\" -U OLD src/main.cpp"
  }}
]"#,
                temp.path().display()
            ),
        )
        .unwrap();

        let database = load_compilation_database(&database).unwrap();
        let command = &database.commands[0];

        assert_eq!(
            command.search_paths.include_dirs,
            vec![temp.path().join("include dir")]
        );
        assert_eq!(command.defines, vec!["MODE=fast"]);
        assert_eq!(command.undefines, vec!["OLD"]);
        assert!(command.raw_command.is_some());
    }

    #[test]
    fn reports_missing_required_fields() {
        let temp = tempdir().unwrap();
        let database = temp.path().join("compile_commands.json");
        fs::write(
            &database,
            r#"[{"file":"src/main.cpp","arguments":["c++"]}]"#,
        )
        .unwrap();

        let error = load_compilation_database(&database)
            .unwrap_err()
            .to_string();

        assert!(error.contains("entry 0 is missing `directory`"));
    }
}
