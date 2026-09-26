//! `.dockerignore` matching with Docker's semantics, ported from moby/patternmatcher: patterns
//! are anchored at the context root, `*` and `?` stop at `/`, `**` spans directories, a pattern
//! also excludes everything under a directory it matches, `!` re-includes, and the last match wins.

use crate::error::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};
use regex::Regex;
use std::path::Path;

#[derive(Debug)]
pub(crate) struct DockerIgnore {
    patterns: Vec<Pattern>,
    has_exclusions: bool,
}

#[derive(Debug)]
struct Pattern {
    matcher: Matcher,
    exclusion: bool,
}

#[derive(Debug)]
enum Matcher {
    Exact(String),
    Prefix(String),
    Suffix(String),
    Regex(Regex),
}

impl DockerIgnore {
    /// Parses an ignore file's contents. `origin` names the file in errors.
    pub(crate) fn parse(contents: &str, origin: &Path) -> Result<Self> {
        let invalid = |pattern: &str| ErrorData::InvalidResourceConfig {
            resource_id: origin.display().to_string(),
            reason: format!("Invalid ignore pattern '{pattern}'"),
        };

        let mut patterns = Vec::new();
        let contents = contents.strip_prefix('\u{feff}').unwrap_or(contents);
        for line in contents.lines() {
            if line.starts_with('#') {
                continue;
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (exclusion, pattern) = match line.strip_prefix('!') {
                Some(rest) => (true, rest.trim()),
                None => (false, line),
            };
            if pattern.is_empty() {
                return Err(AlienError::new(invalid(line)));
            }
            let pattern = clean(pattern);
            let pattern = match pattern.strip_prefix('/') {
                Some(relative) if !relative.is_empty() => relative.to_string(),
                _ => pattern,
            };
            let matcher = compile(&pattern)
                .into_alien_error()
                .context(invalid(line))?;
            patterns.push(Pattern { matcher, exclusion });
        }

        let has_exclusions = patterns.iter().any(|pattern| pattern.exclusion);
        Ok(Self {
            patterns,
            has_exclusions,
        })
    }

    pub(crate) fn empty() -> Self {
        Self {
            patterns: Vec::new(),
            has_exclusions: false,
        }
    }

    /// Whether a `!` pattern could re-include something under an excluded directory.
    pub(crate) fn has_exclusions(&self) -> bool {
        self.has_exclusions
    }

    /// Whether Docker leaves `path` (relative, `/`-separated) out of the build context.
    pub(crate) fn excludes(&self, path: &str) -> bool {
        let parents = path
            .rsplit_once('/')
            .map(|(parent, _)| parent.split('/').collect::<Vec<_>>())
            .unwrap_or_default();

        let mut excluded = false;
        for pattern in &self.patterns {
            if pattern.exclusion != excluded {
                continue;
            }
            let matched = pattern.matcher.matches(path)
                || (1..=parents.len())
                    .any(|depth| pattern.matcher.matches(&parents[..depth].join("/")));
            if matched {
                excluded = !pattern.exclusion;
            }
        }
        excluded
    }
}

impl Matcher {
    fn matches(&self, path: &str) -> bool {
        match self {
            Self::Exact(pattern) => path == pattern,
            Self::Prefix(prefix) => path.starts_with(prefix.as_str()),
            Self::Suffix(suffix) => {
                path.ends_with(suffix.as_str())
                    || suffix.strip_prefix('/').is_some_and(|bare| path == bare)
            }
            Self::Regex(regex) => regex.is_match(path),
        }
    }
}

/// Mirrors patternmatcher's `compile`, including its exact, prefix and suffix shortcuts, which
/// do not always agree with the regex it would otherwise build (`**foo` matches `barfoo`).
fn compile(pattern: &str) -> std::result::Result<Matcher, regex::Error> {
    #[derive(PartialEq)]
    enum Kind {
        Exact,
        Prefix,
        Suffix,
        Regex,
    }

    let mut regex = String::from("^");
    let mut kind = Kind::Exact;
    let mut chars = pattern.chars().peekable();
    let mut first = true;
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                }
                if chars.peek().is_none() {
                    if kind == Kind::Exact {
                        kind = Kind::Prefix;
                    } else {
                        regex.push_str(".*");
                        kind = Kind::Regex;
                    }
                } else {
                    regex.push_str("(.*/)?");
                    kind = Kind::Regex;
                }
                if first {
                    kind = Kind::Suffix;
                }
            }
            '*' => {
                regex.push_str("[^/]*");
                kind = Kind::Regex;
            }
            '?' => {
                regex.push_str("[^/]");
                kind = Kind::Regex;
            }
            '.' | '+' | '(' | ')' | '|' | '{' | '}' | '$' => {
                regex.push('\\');
                regex.push(ch);
            }
            '\\' => match chars.next() {
                Some(escaped) => {
                    regex.push('\\');
                    regex.push(escaped);
                    kind = Kind::Regex;
                }
                None => regex.push('\\'),
            },
            '[' | ']' => {
                regex.push(ch);
                kind = Kind::Regex;
            }
            _ => regex.push(ch),
        }
        first = false;
    }

    Ok(match kind {
        Kind::Exact => Matcher::Exact(pattern.to_string()),
        Kind::Prefix => Matcher::Prefix(pattern[..pattern.len() - 2].to_string()),
        Kind::Suffix => Matcher::Suffix(pattern[2..].to_string()),
        Kind::Regex => {
            regex.push('$');
            Matcher::Regex(Regex::new(&regex)?)
        }
    })
}

/// Go's `path.Clean`: drops empty and `.` segments and resolves `..` lexically.
fn clean(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => match segments.last() {
                Some(&last) if last != ".." => {
                    segments.pop();
                }
                _ if rooted => {}
                _ => segments.push(".."),
            },
            _ => segments.push(segment),
        }
    }
    let joined = segments.join("/");
    match (rooted, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignore(contents: &str) -> DockerIgnore {
        DockerIgnore::parse(contents, Path::new(".dockerignore")).expect("valid ignore file")
    }

    #[test]
    fn matches_like_docker() {
        let cases: &[(&str, &str, bool)] = &[
            ("node_modules", "node_modules/dep/index.js", true),
            ("node_modules", "app/node_modules/dep/index.js", false),
            ("*.log", "a.log", true),
            ("*.log", "sub/a.log", false),
            ("**/*.log", "sub/deep/a.log", true),
            ("**/*.log", "a.log", true),
            ("/build/", "build/out.bin", true),
            ("./app/../secrets", "secrets/key", true),
            ("docs/**", "docs/a/b.md", true),
            ("docs/**", "docsx", false),
            ("**foo", "barfoo", true),
            ("a?c", "abc", true),
            ("a?c", "a/c", false),
            ("[ab].txt", "b.txt", true),
            ("[ab].txt", "c.txt", false),
            ("**", "anything/at/all", true),
        ];
        for (pattern, path, excluded) in cases {
            assert_eq!(
                ignore(pattern).excludes(path),
                *excluded,
                "pattern {pattern:?} on {path:?}"
            );
        }
    }

    #[test]
    fn the_last_matching_line_wins() {
        let rules = ignore("# comment\n*.md\n!README.md\nREADME.md.bak\n");
        assert!(rules.excludes("CHANGES.md"));
        assert!(!rules.excludes("README.md"));
        assert!(rules.has_exclusions());

        let rules = ignore("!keep\nkeep");
        assert!(rules.excludes("keep"));
    }

    #[test]
    fn a_reincluded_file_under_an_excluded_directory_is_sent() {
        let rules = ignore("vendor\n!vendor/keep.txt\n");
        assert!(rules.excludes("vendor/drop.txt"));
        assert!(!rules.excludes("vendor/keep.txt"));
    }

    #[test]
    fn a_bad_pattern_is_an_error() {
        DockerIgnore::parse("[unclosed", Path::new(".dockerignore"))
            .expect_err("an unbalanced class is not a pattern");
        DockerIgnore::parse("!", Path::new(".dockerignore"))
            .expect_err("a bare ! excludes nothing");
    }
}
