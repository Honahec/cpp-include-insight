use crate::{
    compile_commands::CompilationDatabase,
    parser::{IncludeDirective, parse_include_line},
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::{
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    pub include_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIncludes {
    pub file: PathBuf,
    pub includes: Vec<IncludeDirective>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub files_scanned: usize,
    pub includes_found: usize,
    pub files: Vec<FileIncludes>,
}

pub fn scan_project(root: impl AsRef<Path>, _options: &ScanOptions) -> Result<ScanResult> {
    let root = root.as_ref();
    let mut result = ScanResult::default();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !should_ignore(entry.path()))
    {
        let entry = entry?;
        let path = entry.path();

        if !entry.file_type().is_file() || !is_cpp_like_file(path) {
            continue;
        }

        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let includes = content
            .lines()
            .enumerate()
            .filter_map(|(index, line)| parse_include_line(line, index + 1))
            .collect::<Vec<_>>();

        result.files_scanned += 1;
        result.includes_found += includes.len();

        result.files.push(FileIncludes {
            file: path.to_path_buf(),
            includes,
        });
    }

    Ok(result)
}

pub fn scan_compilation_database(database: &CompilationDatabase) -> Result<ScanResult> {
    let mut result = ScanResult::default();
    let mut seen = HashSet::new();

    for command in &database.commands {
        if !seen.insert(normalize_path(&command.file)) {
            continue;
        }

        scan_file(&command.file, &mut result)?;
    }

    Ok(result)
}

fn scan_file(path: &Path, result: &mut ScanResult) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    let includes = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| parse_include_line(line, index + 1))
        .collect::<Vec<_>>();

    result.files_scanned += 1;
    result.includes_found += includes.len();

    result.files.push(FileIncludes {
        file: path.to_path_buf(),
        includes,
    });

    Ok(())
}

fn should_ignore(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    name.starts_with("cmake-build-")
        || matches!(
            name,
            ".git"
                | "build"
                | "cmake_build_release"
                | "target"
                | "node_modules"
                | "third_party"
                | "external"
                | "vendor"
        )
}

fn is_cpp_like_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx")
    )
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn ignores_cmake_build_directories_by_prefix() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let build_dir = temp.path().join("cmake-build-release");

        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&build_dir).unwrap();
        fs::write(src_dir.join("main.cpp"), "#include \"app.h\"\n").unwrap();
        fs::write(
            build_dir.join("generated.cpp"),
            "#include \"generated.h\"\n",
        )
        .unwrap();

        let result = scan_project(temp.path(), &ScanOptions::default()).unwrap();

        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.files[0].file, src_dir.join("main.cpp"));
    }

    #[test]
    fn scans_unique_files_from_compilation_database() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let main_cpp = src_dir.join("main.cpp");
        fs::write(&main_cpp, "#include \"app.h\"\n").unwrap();

        let database = CompilationDatabase {
            commands: vec![
                crate::CompileCommand {
                    directory: temp.path().to_path_buf(),
                    file: main_cpp.clone(),
                    output: None,
                    raw_command: None,
                    arguments: vec!["c++".to_owned(), "src/main.cpp".to_owned()],
                    search_paths: crate::FileSearchPaths::default(),
                    defines: Vec::new(),
                    undefines: Vec::new(),
                },
                crate::CompileCommand {
                    directory: temp.path().to_path_buf(),
                    file: main_cpp,
                    output: None,
                    raw_command: None,
                    arguments: vec!["c++".to_owned(), "src/main.cpp".to_owned()],
                    search_paths: crate::FileSearchPaths::default(),
                    defines: Vec::new(),
                    undefines: Vec::new(),
                },
            ],
        };

        let result = scan_compilation_database(&database).unwrap();

        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.includes_found, 1);
    }
}
