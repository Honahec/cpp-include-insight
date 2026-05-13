use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{FileSearchPaths, IncludeDirective, IncludeKind};

#[derive(Debug, Clone)]
pub struct IncludeResolver {
    root: PathBuf,
    include_dirs: Vec<PathBuf>,
    file_search_paths: HashMap<PathBuf, Vec<FileSearchPaths>>,
    resolve_angle_in_search_paths: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncludeResolution {
    Resolved(PathBuf),
    External(String),
    Missing(String),
}

impl IncludeResolver {
    pub fn new(root: impl Into<PathBuf>, include_dirs: Vec<PathBuf>) -> Self {
        let root = root.into();

        let include_dirs = include_dirs
            .into_iter()
            .map(|include_dir| {
                if include_dir.is_absolute() {
                    include_dir
                } else {
                    root.join(include_dir)
                }
            })
            .collect();

        Self {
            root,
            include_dirs,
            file_search_paths: HashMap::new(),
            resolve_angle_in_search_paths: false,
        }
    }

    pub fn with_file_search_paths(
        root: impl Into<PathBuf>,
        include_dirs: Vec<PathBuf>,
        file_search_paths: HashMap<PathBuf, Vec<FileSearchPaths>>,
    ) -> Self {
        let mut resolver = Self::new(root, include_dirs);
        resolver.file_search_paths = file_search_paths
            .into_iter()
            .map(|(file, paths)| (normalize_path(&file), paths))
            .collect();
        resolver.resolve_angle_in_search_paths = true;
        resolver
    }

    pub fn resolve_include(
        &self,
        including_file: impl AsRef<Path>,
        include: &IncludeDirective,
    ) -> IncludeResolution {
        self.resolve_includes(including_file, include)
            .into_iter()
            .next()
            .unwrap_or_else(|| IncludeResolution::Missing(include.path.clone()))
    }

    pub fn resolve_includes(
        &self,
        including_file: impl AsRef<Path>,
        include: &IncludeDirective,
    ) -> Vec<IncludeResolution> {
        match include.kind {
            IncludeKind::Angle if self.resolve_angle_in_search_paths => {
                self.resolve_angle_include(including_file.as_ref(), include)
            }
            IncludeKind::Angle => vec![IncludeResolution::External(include.path.clone())],
            IncludeKind::Quote => self.resolve_quote_include(including_file.as_ref(), include),
        }
    }

    fn resolve_quote_include(
        &self,
        including_file: &Path,
        include: &IncludeDirective,
    ) -> Vec<IncludeResolution> {
        let include_path = Path::new(&include.path);

        if let Some(parent) = including_file.parent() {
            let candidate = parent.join(include_path);

            if candidate.is_file() {
                return vec![IncludeResolution::Resolved(candidate)];
            }
        }

        let resolutions = self
            .search_path_contexts_for(including_file)
            .into_iter()
            .map(|search_paths| {
                for include_dir in search_paths
                    .quote_dirs
                    .iter()
                    .chain(search_paths.include_dirs.iter())
                    .chain(search_paths.system_dirs.iter())
                {
                    let candidate = include_dir.join(include_path);

                    if candidate.is_file() {
                        return self.resolve_candidate_or_external(candidate, include);
                    }
                }

                IncludeResolution::Missing(include.path.clone())
            })
            .collect::<Vec<_>>();

        dedupe_resolutions(resolutions)
    }

    fn resolve_angle_include(
        &self,
        including_file: &Path,
        include: &IncludeDirective,
    ) -> Vec<IncludeResolution> {
        let include_path = Path::new(&include.path);
        let resolutions = self
            .search_path_contexts_for(including_file)
            .into_iter()
            .map(|search_paths| {
                for include_dir in search_paths
                    .include_dirs
                    .iter()
                    .chain(search_paths.system_dirs.iter())
                {
                    let candidate = include_dir.join(include_path);

                    if candidate.is_file() {
                        return self.resolve_candidate_or_external(candidate, include);
                    }
                }

                IncludeResolution::External(include.path.clone())
            })
            .collect::<Vec<_>>();

        dedupe_resolutions(resolutions)
    }

    fn resolve_candidate_or_external(
        &self,
        candidate: PathBuf,
        include: &IncludeDirective,
    ) -> IncludeResolution {
        let candidate = normalize_path(&candidate);

        if is_under_root(&candidate, &self.root) {
            IncludeResolution::Resolved(candidate)
        } else {
            IncludeResolution::External(include.path.clone())
        }
    }

    fn search_path_contexts_for(&self, including_file: &Path) -> Vec<FileSearchPaths> {
        let mut contexts = self
            .file_search_paths
            .get(&normalize_path(including_file))
            .cloned()
            .unwrap_or_else(|| vec![FileSearchPaths::default()]);

        if contexts.is_empty() {
            contexts.push(FileSearchPaths::default());
        }

        for search_paths in &mut contexts {
            search_paths.extend_include_dirs(self.include_dirs.clone());
        }

        contexts
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn include_dirs(&self) -> &[PathBuf] {
        &self.include_dirs
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_under_root(path: &Path, root: &Path) -> bool {
    let path = normalize_path(path);
    let root = normalize_path(root);
    path.starts_with(root)
}

fn dedupe_resolutions(resolutions: Vec<IncludeResolution>) -> Vec<IncludeResolution> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for resolution in resolutions {
        if seen.insert(resolution.clone()) {
            deduped.push(resolution);
        }
    }

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_include_line;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolves_quote_include_from_current_file_directory() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        let local_h = src_dir.join("local.h");

        fs::write(&main_cpp, "").unwrap();
        fs::write(&local_h, "").unwrap();

        let resolver = IncludeResolver::new(temp.path(), vec![]);
        let include = parse_include_line(r#"#include "local.h""#, 1).unwrap();

        let resolution = resolver.resolve_include(&main_cpp, &include);

        assert_eq!(resolution, IncludeResolution::Resolved(local_h))
    }

    #[test]
    fn resolves_quote_include_from_include_dir() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let include_dir = temp.path().join("include");

        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&include_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        let app_h = include_dir.join("app.h");

        fs::write(&main_cpp, "").unwrap();
        fs::write(&app_h, "").unwrap();

        let resolver = IncludeResolver::new(temp.path(), vec![PathBuf::from("include")]);
        let include = parse_include_line(r#"#include "app.h""#, 1).unwrap();

        let resolution = resolver.resolve_include(&main_cpp, &include);

        assert_eq!(resolution, IncludeResolution::Resolved(app_h));
    }

    #[test]
    fn resolves_quote_include_from_compile_command_iquote_before_include_dir() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let quote_dir = temp.path().join("quote");
        let include_dir = temp.path().join("include");

        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&quote_dir).unwrap();
        fs::create_dir_all(&include_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        let quote_app = quote_dir.join("app.h");
        let include_app = include_dir.join("app.h");

        fs::write(&main_cpp, "").unwrap();
        fs::write(&quote_app, "").unwrap();
        fs::write(&include_app, "").unwrap();

        let resolver = IncludeResolver::with_file_search_paths(
            temp.path(),
            vec![],
            HashMap::from([(
                main_cpp.clone(),
                vec![FileSearchPaths {
                    quote_dirs: vec![quote_dir],
                    include_dirs: vec![include_dir],
                    system_dirs: vec![],
                }],
            )]),
        );
        let include = parse_include_line(r#"#include "app.h""#, 1).unwrap();

        let resolution = resolver.resolve_include(&main_cpp, &include);

        assert_eq!(resolution, IncludeResolution::Resolved(quote_app));
    }

    #[test]
    fn resolves_angle_include_from_compile_command_include_dir() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let include_dir = temp.path().join("include");

        fs::create_dir_all(&src_dir).unwrap();
        fs::create_dir_all(&include_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        let api_h = include_dir.join("project/api.h");

        fs::create_dir_all(api_h.parent().unwrap()).unwrap();
        fs::write(&main_cpp, "").unwrap();
        fs::write(&api_h, "").unwrap();

        let resolver = IncludeResolver::with_file_search_paths(
            temp.path(),
            vec![],
            HashMap::from([(
                main_cpp.clone(),
                vec![FileSearchPaths {
                    quote_dirs: vec![],
                    include_dirs: vec![include_dir],
                    system_dirs: vec![],
                }],
            )]),
        );
        let include = parse_include_line("#include <project/api.h>", 1).unwrap();

        let resolution = resolver.resolve_include(&main_cpp, &include);

        assert_eq!(resolution, IncludeResolution::Resolved(api_h));
    }

    #[test]
    fn marks_missing_quote_include_as_missing() {
        let temp = tempdir().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let main_cpp = src_dir.join("main.cpp");
        fs::write(&main_cpp, "").unwrap();

        let resolver = IncludeResolver::new(temp.path(), vec![]);
        let include = parse_include_line(r#"#include "missing.h""#, 1).unwrap();

        let resolution = resolver.resolve_include(&main_cpp, &include);

        assert_eq!(
            resolution,
            IncludeResolution::Missing("missing.h".to_owned())
        );
    }

    #[test]
    fn marks_angle_include_as_external() {
        let temp = tempdir().unwrap();
        let main_cpp = temp.path().join("main.cpp");
        fs::write(&main_cpp, "").unwrap();

        let resolver = IncludeResolver::new(temp.path(), vec![]);
        let include = parse_include_line("#include <vector>", 1).unwrap();

        let resolution = resolver.resolve_include(&main_cpp, &include);

        assert_eq!(resolution, IncludeResolution::External("vector".to_owned()));
    }

    #[test]
    fn stores_absolute_include_dirs_as_is() {
        let temp = tempdir().unwrap();
        let include_dir = temp.path().join("include");

        let resolver = IncludeResolver::new(temp.path(), vec![include_dir.clone()]);

        assert_eq!(resolver.include_dirs(), &[include_dir]);
    }

    #[test]
    fn resolves_relative_include_dirs_from_root() {
        let temp = tempdir().unwrap();

        let resolver = IncludeResolver::new(temp.path(), vec![PathBuf::from("include")]);

        assert_eq!(resolver.include_dirs(), &[temp.path().join("include")]);
    }
}
