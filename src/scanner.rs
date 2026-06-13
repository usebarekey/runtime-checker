use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use anyhow::{Context, Result};
use fff_grep::{
    LineTerminator, Match, Matcher, NoError, Searcher, SearcherBuilder, Sink, SinkMatch,
};
use ignore::WalkBuilder;

use crate::{
    data::{Feature, RuntimeDb},
    version::RuntimeVersion,
};

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanStats {
    pub line_count: usize,
}

#[derive(Debug, Clone)]
pub struct SourceScan {
    pub files: Vec<SourceFile>,
    pub stats: ScanStats,
}

#[derive(Debug, Clone)]
pub struct DetectedFeature {
    pub feature: String,
    pub version: RuntimeVersion,
    pub path: PathBuf,
    pub line: u64,
    pub column: u64,
    pub count: usize,
}

pub trait Scanner {
    type Output;

    fn scan(&self, root: &Path, runtime: &RuntimeDb) -> Result<Self::Output>;
}

pub struct SourceDiscovery;

impl Scanner for SourceDiscovery {
    type Output = SourceScan;

    fn scan(&self, root: &Path, _runtime: &RuntimeDb) -> Result<Self::Output> {
        let mut files = Vec::new();
        let mut stats = ScanStats::default();
        let mut builder = WalkBuilder::new(root);
        builder.filter_entry(|entry| !is_ignored_dir(entry.path()));

        for entry in builder.build() {
            let entry = entry?;
            let path = entry.path();
            if !entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
            {
                continue;
            }
            if !is_source_file(path) {
                continue;
            }

            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            stats.line_count += count_lines(&text);
            files.push(SourceFile {
                path: path.to_path_buf(),
                text,
            });
        }

        Ok(SourceScan { files, stats })
    }
}

#[derive(Clone, Copy)]
struct RuntimePattern<'a> {
    runtime_index: usize,
    runtime: &'a RuntimeDb,
    feature: &'a Feature,
}

pub struct FffMultiRuntimeScanner;

impl FffMultiRuntimeScanner {
    pub fn scan_files(
        &self,
        runtimes: &[&RuntimeDb],
        files: &[SourceFile],
    ) -> Result<Vec<Vec<DetectedFeature>>> {
        let entries = combined_fast_patterns(runtimes);
        if entries.is_empty() {
            return Ok(vec![Vec::new(); runtimes.len()]);
        }

        let pattern_refs: Vec<&str> = entries.iter().map(|(pattern, _)| *pattern).collect();
        let matcher = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(pattern_refs)
            .context("failed to build matcher")?;
        let searcher = SearcherBuilder::new().line_number(true).build();

        let mut detections_by_runtime = vec![Vec::new(); runtimes.len()];
        let mut seen_by_runtime = vec![HashSet::new(); runtimes.len()];
        for (file_index, file) in files.iter().enumerate() {
            let mut sink = FffMultiRuntimeSink {
                entries: &entries,
                matcher: &matcher,
                file_index,
                path: &file.path,
                detections_by_runtime: &mut detections_by_runtime,
                seen_by_runtime: &mut seen_by_runtime,
            };
            searcher
                .search_slice(
                    FffAhoMatcher { matcher: &matcher },
                    file.text.as_bytes(),
                    &mut sink,
                )
                .context("failed to search source with FFF")?;
        }

        Ok(detections_by_runtime)
    }
}

fn combined_fast_patterns<'a>(
    runtimes: &[&'a RuntimeDb],
) -> Vec<(&'a str, Vec<RuntimePattern<'a>>)> {
    let mut by_pattern = HashMap::<&'a str, Vec<RuntimePattern<'a>>>::new();
    for (runtime_index, runtime) in runtimes.iter().copied().enumerate() {
        for &pattern in runtime.fast_patterns() {
            let Some(feature) = runtime.feature_for_pattern(pattern) else {
                continue;
            };
            by_pattern.entry(pattern).or_default().push(RuntimePattern {
                runtime_index,
                runtime,
                feature,
            });
        }
    }

    let mut entries: Vec<_> = by_pattern.into_iter().collect();
    entries.sort_by(|left, right| {
        right
            .0
            .len()
            .cmp(&left.0.len())
            .then_with(|| left.0.cmp(right.0))
    });
    entries
}

struct FffAhoMatcher<'a> {
    matcher: &'a AhoCorasick,
}

impl Matcher for FffAhoMatcher<'_> {
    type Error = NoError;

    fn find_at(&self, haystack: &[u8], at: usize) -> std::result::Result<Option<Match>, NoError> {
        Ok(self
            .matcher
            .find(&haystack[at..])
            .map(|matched| Match::new(at + matched.start(), at + matched.end())))
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        Some(LineTerminator::byte(b'\n'))
    }
}

struct FffMultiRuntimeSink<'a> {
    entries: &'a [(&'a str, Vec<RuntimePattern<'a>>)],
    matcher: &'a AhoCorasick,
    file_index: usize,
    path: &'a Path,
    detections_by_runtime: &'a mut [Vec<DetectedFeature>],
    seen_by_runtime: &'a mut [DetectionSeen],
}

impl Sink for FffMultiRuntimeSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, matched: &SinkMatch<'_>) -> io::Result<bool> {
        let Ok(line) = std::str::from_utf8(matched.bytes()) else {
            return Ok(true);
        };
        let line = line.trim_end_matches(['\r', '\n']);
        let line_number = matched.line_number().unwrap_or(1);

        for found in self.matcher.find_iter(line) {
            for runtime_pattern in &self.entries[found.pattern()].1 {
                if !is_fast_match(
                    runtime_pattern.runtime,
                    line,
                    found.start(),
                    found.end(),
                    self.entries[found.pattern()].0,
                ) {
                    continue;
                }
                push_detection(
                    &mut self.detections_by_runtime[runtime_pattern.runtime_index],
                    &mut self.seen_by_runtime[runtime_pattern.runtime_index],
                    runtime_pattern.feature,
                    self.file_index,
                    self.path,
                    line_number,
                    (found.start() + 1) as u64,
                );
            }
        }

        Ok(true)
    }
}

impl SourceScan {
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn stats(&self) -> ScanStats {
        self.stats
    }
}

pub type DetectionSeen = HashSet<(usize, usize, u64, u64)>;

pub fn push_detection(
    detections: &mut Vec<DetectedFeature>,
    seen: &mut DetectionSeen,
    feature: &Feature,
    file_index: usize,
    path: &Path,
    line: u64,
    column: u64,
) {
    let key = (feature.id, file_index, line, column);
    if seen.insert(key) {
        detections.push(DetectedFeature {
            feature: feature.name.to_owned(),
            version: feature.version,
            path: path.to_path_buf(),
            line,
            column,
            count: 1,
        });
    }
}

pub fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts")
    )
}

fn is_ignored_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git" | "node_modules" | "dist" | "build" | "coverage" | "target"
    )
}

fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.lines().count()
    }
}

fn is_fast_match(runtime: &RuntimeDb, line: &str, start: usize, end: usize, pattern: &str) -> bool {
    (runtime.is_property_pattern(pattern) && is_property_access(line, start, end))
        || (runtime.is_global_or_member_pattern(pattern)
            && is_safe_global_or_member_pattern(pattern)
            && has_identifier_boundaries(line, start, end))
}

fn is_safe_global_or_member_pattern(pattern: &str) -> bool {
    pattern.contains('.')
        || pattern
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
        || matches!(
            pattern,
            "alert"
                | "atob"
                | "btoa"
                | "caches"
                | "cancelAnimationFrame"
                | "cancelIdleCallback"
                | "clearImmediate"
                | "clearInterval"
                | "clearTimeout"
                | "console"
                | "crypto"
                | "document"
                | "fetch"
                | "global"
                | "globalThis"
                | "indexedDB"
                | "localStorage"
                | "location"
                | "navigator"
                | "performance"
                | "process"
                | "queueMicrotask"
                | "reportError"
                | "requestAnimationFrame"
                | "requestIdleCallback"
                | "self"
                | "sessionStorage"
                | "setImmediate"
                | "setInterval"
                | "setTimeout"
                | "structuredClone"
                | "window"
        )
}

fn is_property_access(line: &str, start: usize, end: usize) -> bool {
    previous_char(line, start) == Some('.')
        && !next_char(line, end).is_some_and(is_js_identifier_part)
}

fn has_identifier_boundaries(line: &str, start: usize, end: usize) -> bool {
    !previous_char(line, start).is_some_and(|ch| is_js_identifier_part(ch) || ch == '.')
        && !next_char(line, end).is_some_and(is_js_identifier_part)
}

fn previous_char(text: &str, index: usize) -> Option<char> {
    text.get(..index)?.chars().next_back()
}

fn next_char(text: &str, index: usize) -> Option<char> {
    text.get(index..)?.chars().next()
}

fn is_js_identifier_part(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::{data::node_runtime, scanner::Scanner};

    use super::SourceDiscovery;

    #[test]
    fn source_discovery_filters_before_scanners_run() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("app.ts"), "Temporal.Now.instant();\n").unwrap();
        fs::write(dir.path().join("README.md"), "Temporal.Now.instant();\n").unwrap();

        for ignored in [
            "node_modules",
            ".git",
            "dist",
            "build",
            "coverage",
            "target",
        ] {
            let ignored_dir = dir.path().join(ignored);
            fs::create_dir(&ignored_dir).unwrap();
            fs::write(ignored_dir.join("ignored.ts"), "Temporal.Now.instant();\n").unwrap();
        }

        let scan = SourceDiscovery
            .scan(dir.path(), node_runtime().unwrap())
            .unwrap();

        assert_eq!(scan.stats().line_count, 1);
        assert_eq!(scan.files().len(), 1);
        assert_eq!(scan.files()[0].path.file_name().unwrap(), "app.ts");
    }
}
