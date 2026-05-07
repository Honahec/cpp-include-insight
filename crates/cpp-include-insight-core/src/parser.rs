use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

// #include "app.h" // comment
static INCLUDE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*#\s*include\s*([<"])([^>"]+)[>"]\s*(?://.*)?$"#).expect("valid include regax")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncludeKind {
    Quote,
    Angle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncludeDirective {
    pub kind: IncludeKind,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub raw: String,
}

pub fn parse_include_line(line: &str, line_number: usize) -> Option<IncludeDirective> {
    let captures = INCLUDE_RE.captures(line)?;

    let opener = captures.get(1)?;
    let include_path = captures.get(2)?;

    let kind = match opener.as_str() {
        "\"" => IncludeKind::Quote,
        "<" => IncludeKind::Angle,
        _ => return None,
    };

    Some(IncludeDirective {
        kind,
        path: include_path.as_str().to_owned(),
        line: line_number,
        column: opener.start() + 1,
        raw: line.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quote_include() {
        let include = parse_include_line(r#"#include "app.h""#, 1).unwrap();

        assert_eq!(include.kind, IncludeKind::Quote);
        assert_eq!(include.path, "app.h");
        assert_eq!(include.line, 1);
    }

    #[test]
    fn parses_angle_include() {
        let include = parse_include_line("#include <vector>", 7).unwrap();

        assert_eq!(include.kind, IncludeKind::Angle);
        assert_eq!(include.path, "vector");
        assert_eq!(include.line, 7);
    }

    #[test]
    fn parses_spaced_include() {
        let include = parse_include_line(r#"# include "foo/bar.hpp""#, 3).unwrap();

        assert_eq!(include.kind, IncludeKind::Quote);
        assert_eq!(include.path, "foo/bar.hpp");
    }

    #[test]
    fn parses_comment_include() {
        let include = parse_include_line(r#"#include "app.h" // comment"#, 2).unwrap();

        assert_eq!(include.kind, IncludeKind::Quote);
        assert_eq!(include.path, "app.h");
    }

    #[test]
    fn ignores_non_include_line() {
        assert!(parse_include_line("int main() { return 0; }", 1).is_none());
    }
}
