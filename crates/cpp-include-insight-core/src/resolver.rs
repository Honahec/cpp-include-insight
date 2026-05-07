use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::{IncludeDirective, IncludeKind};

#[derive(Debug, Clone)]
pub struct IncludeResolver {
    root: PathBuf,
    include_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

        Self { root, include_dirs }
    }

    pub fn resolve_include(
        &self,
        including_file: impl AsRef<Path>,
        include: &IncludeDirective,
    ) -> IncludeResolution {
        match include.kind {
            IncludeKind::Angle => IncludeResolution::External(include.path.clone()),
            IncludeKind::Quote => self.resolve_quote_include(including_file.as_ref(), include),
        }
    }

    fn resolve_quote_include(
        &self,
        including_file: &Path,
        include: &IncludeDirective,
    ) -> IncludeResolution {
        let include_path = Path::new(&include.path);

        if let Some(parent) = including_file.parent() {
            let candidate = parent.join(include_path);

            if candidate.is_file() {
                return IncludeResolution::Resolved(candidate);
            }
        }

        for include_dir in &self.include_dirs {
            let candidate = include_dir.join(include_path);

            if candidate.is_file() {
                return IncludeResolution::Resolved(candidate);
            }
        }

        IncludeResolution::Missing(include.path.clone())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn include_dirs(&self) -> &[PathBuf] {
        &self.include_dirs
    }
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
