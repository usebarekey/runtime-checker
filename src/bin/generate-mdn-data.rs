use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result};
use clap::Parser;
use serde_json::Value;

const DEFAULT_BCD_VERSION: &str = include_str!("../../data/mdn-bcd.version");
const DEFAULT_OUTPUT_DIR: &str = "data/mdn";

#[derive(Debug, Parser)]
#[command(name = "generate-mdn-data")]
#[command(about = "Generate runtime RON data from MDN browser-compat-data")]
struct Args {
    /// Read MDN BCD data.json from disk instead of downloading it.
    #[arg(long)]
    input: Option<PathBuf>,

    /// MDN @mdn/browser-compat-data version to download.
    #[arg(long)]
    bcd_version: Option<String>,

    /// Directory where runtime RON files are written.
    #[arg(long, default_value = DEFAULT_OUTPUT_DIR)]
    output_dir: PathBuf,

    /// Runtime ids to generate: nodejs, deno, bun, safari, chrome, firefox.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = "nodejs,deno,bun,safari,chrome,firefox"
    )]
    runtimes: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    run(args)
}

fn run(args: Args) -> Result<()> {
    let bcd_version = args
        .bcd_version
        .unwrap_or_else(|| DEFAULT_BCD_VERSION.trim().to_string());
    let input = load_bcd_json(args.input.as_deref(), &bcd_version)?;
    let bcd: Value = serde_json::from_str(&input).context("failed to parse MDN BCD data.json")?;

    if let Some(actual) = bcd
        .pointer("/__meta/version")
        .and_then(Value::as_str)
        .filter(|actual| *actual != bcd_version)
    {
        anyhow::bail!("expected BCD {bcd_version}, got {actual}");
    }

    fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("failed to create {}", args.output_dir.display()))?;

    for runtime_id in args.runtimes {
        let target = RuntimeTarget::from_bcd_id(&runtime_id)?;
        let features = extract_features(&bcd, target.bcd_id)?;
        let output =
            render_runtime_ron(target.runtime_name, target.bcd_id, &bcd_version, &features);
        let path = args.output_dir.join(format!("{}.ron", target.runtime_name));
        fs::write(&path, output).with_context(|| format!("failed to write {}", path.display()))?;
        println!(
            "generated {} features for {} from MDN BCD {} -> {}",
            features.len(),
            target.runtime_name,
            bcd_version,
            path.display()
        );
    }

    Ok(())
}

fn load_bcd_json(input: Option<&Path>, version: &str) -> Result<String> {
    if let Some(input) = input {
        return fs::read_to_string(input)
            .with_context(|| format!("failed to read {}", input.display()));
    }

    let url = format!("https://unpkg.com/@mdn/browser-compat-data@{version}/data.json");
    let response = ureq::get(&url)
        .call()
        .with_context(|| format!("failed to download {url}"))?;
    let mut text = String::new();
    response
        .into_reader()
        .read_to_string(&mut text)
        .context("failed to read downloaded MDN BCD data.json")?;
    Ok(text)
}

#[derive(Debug, Clone, Copy)]
struct RuntimeTarget {
    bcd_id: &'static str,
    runtime_name: &'static str,
}

impl RuntimeTarget {
    fn from_bcd_id(id: &str) -> Result<Self> {
        match id {
            "nodejs" | "node" => Ok(Self {
                bcd_id: "nodejs",
                runtime_name: "node",
            }),
            "deno" => Ok(Self {
                bcd_id: "deno",
                runtime_name: "deno",
            }),
            "bun" => Ok(Self {
                bcd_id: "bun",
                runtime_name: "bun",
            }),
            "safari" => Ok(Self {
                bcd_id: "safari",
                runtime_name: "safari",
            }),
            "chrome" => Ok(Self {
                bcd_id: "chrome",
                runtime_name: "chrome",
            }),
            "firefox" => Ok(Self {
                bcd_id: "firefox",
                runtime_name: "firefox",
            }),
            other => anyhow::bail!("unsupported BCD runtime id `{other}`"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedFeature {
    name: String,
    version: Version,
    detect: Vec<GeneratedDetectRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl FromStr for Version {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let value = value.trim().trim_start_matches('v');
        if !value
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit())
        {
            anyhow::bail!("version is not numeric: {value}");
        }

        let mut parts = value.split('.');
        let major = parse_version_part(parts.next().unwrap_or("0"), value, "major")?;
        let minor = parse_version_part(parts.next().unwrap_or("0"), value, "minor")?;
        let patch = parse_version_part(parts.next().unwrap_or("0"), value, "patch")?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn parse_version_part(part: &str, full: &str, label: &str) -> Result<u64> {
    if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("invalid {label} version in `{full}`");
    }
    part.parse()
        .with_context(|| format!("invalid {label} version in `{full}`"))
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum GeneratedDetectRule {
    Global(String),
    MemberChain(String),
    Property(String),
}

fn extract_features(bcd: &Value, runtime_id: &str) -> Result<Vec<GeneratedFeature>> {
    let mut features = BTreeMap::<String, GeneratedFeature>::new();

    if let Some(api) = bcd.get("api") {
        walk_compat_tree(
            api,
            &mut Vec::new(),
            SourceKind::Api,
            runtime_id,
            &mut features,
        );
    }

    if let Some(builtins) = bcd.pointer("/javascript/builtins") {
        walk_compat_tree(
            builtins,
            &mut Vec::new(),
            SourceKind::JavascriptBuiltin,
            runtime_id,
            &mut features,
        );
    }

    Ok(features.into_values().collect())
}

#[derive(Debug, Clone, Copy)]
enum SourceKind {
    Api,
    JavascriptBuiltin,
}

fn walk_compat_tree(
    value: &Value,
    path: &mut Vec<String>,
    source: SourceKind,
    runtime_id: &str,
    features: &mut BTreeMap<String, GeneratedFeature>,
) {
    let Some(object) = value.as_object() else {
        return;
    };

    if let Some(compat) = object.get("__compat")
        && let Some(version) = support_version(compat, runtime_id)
    {
        let name = feature_name(path);
        if !name.is_empty() && is_runtime_surface_path(path) {
            let detect = detect_rules(source, path);
            if !detect.is_empty() {
                upsert_feature(
                    features,
                    GeneratedFeature {
                        name,
                        version,
                        detect,
                    },
                );
            }
        }
    }

    for (key, child) in object {
        if key == "__compat" {
            continue;
        }
        path.push(key.clone());
        walk_compat_tree(child, path, source, runtime_id, features);
        path.pop();
    }
}

fn upsert_feature(features: &mut BTreeMap<String, GeneratedFeature>, feature: GeneratedFeature) {
    match features.get(&feature.name) {
        Some(existing) if existing.version <= feature.version => {}
        _ => {
            features.insert(feature.name.clone(), feature);
        }
    }
}

fn support_version(compat: &Value, runtime_id: &str) -> Option<Version> {
    let support = compat.get("support")?.get(runtime_id)?;
    let statements: Vec<&Value> = match support {
        Value::Array(values) => values.iter().collect(),
        value => vec![value],
    };

    statements
        .into_iter()
        .filter(|statement| !has_runtime_flags(statement))
        .filter(|statement| !has_version_removed(statement))
        .filter_map(|statement| statement.get("version_added")?.as_str())
        .filter_map(|version| version.parse::<Version>().ok())
        .min()
}

fn has_runtime_flags(statement: &Value) -> bool {
    statement
        .get("flags")
        .and_then(Value::as_array)
        .is_some_and(|flags| !flags.is_empty())
}

fn has_version_removed(statement: &Value) -> bool {
    statement
        .get("version_removed")
        .is_some_and(|version| !version.is_null())
}

fn feature_name(path: &[String]) -> String {
    path.join(".")
}

fn is_runtime_surface_path(path: &[String]) -> bool {
    if path.is_empty() {
        return false;
    }

    if path.len() > 1 && path.first() == path.last() {
        return false;
    }

    path.iter()
        .all(|segment| is_runtime_surface_segment(segment))
}

fn is_runtime_surface_segment(segment: &str) -> bool {
    if segment.is_empty() || segment.starts_with("@@") || segment.contains('-') {
        return false;
    }

    if segment.contains('_') {
        return segment
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_digit() || ch.is_ascii_uppercase());
    }

    segment
        .chars()
        .all(|ch| ch == '$' || ch.is_ascii_alphanumeric())
}

fn detect_rules(source: SourceKind, path: &[String]) -> Vec<GeneratedDetectRule> {
    if path.is_empty() {
        return Vec::new();
    }

    let name = feature_name(path);
    let mut rules = BTreeSet::new();

    if path.len() == 1 {
        rules.insert(GeneratedDetectRule::Global(name.clone()));
    }

    rules.insert(GeneratedDetectRule::MemberChain(name));

    if matches!(source, SourceKind::JavascriptBuiltin)
        && path.len() >= 2
        && let Some(property) = path
            .last()
            .filter(|property| is_detectable_property(property))
    {
        rules.insert(GeneratedDetectRule::Property(property.clone()));
    }

    rules.into_iter().collect()
}

fn is_detectable_property(property: &str) -> bool {
    !property.starts_with("@@")
        && !property.starts_with("__")
        && property
            .chars()
            .next()
            .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
}

fn render_runtime_ron(
    runtime_name: &str,
    bcd_id: &str,
    bcd_version: &str,
    features: &[GeneratedFeature],
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "//! Generated from MDN @mdn/browser-compat-data {bcd_version} runtime `{bcd_id}`.\n"
    ));
    output.push_str(
        "//! Regenerate with: cargo run --bin generate-mdn-data -- --output-dir data/mdn\n",
    );
    output.push_str("(\n");
    output.push_str("    schema: 1,\n");
    output.push_str(&format!("    runtime: {},\n", quote(runtime_name)));
    output.push_str("    features: [\n");

    for feature in features {
        output.push_str("        (name: ");
        output.push_str(&quote(&feature.name));
        output.push_str(", version: ");
        output.push_str(&quote(&feature.version.to_string()));
        output.push_str(", detect: [");
        for (index, rule) in feature.detect.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            output.push_str(&render_detect_rule(rule));
        }
        output.push_str("]),\n");
    }

    output.push_str("    ],\n");
    output.push_str(")\n");
    output
}

fn render_detect_rule(rule: &GeneratedDetectRule) -> String {
    match rule {
        GeneratedDetectRule::Global(name) => format!("Global({})", quote(name)),
        GeneratedDetectRule::MemberChain(name) => format!("MemberChain({})", quote(name)),
        GeneratedDetectRule::Property(name) => format!("Property({})", quote(name)),
    }
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("string escaping cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_runtime_versions_from_bcd_shape() {
        let bcd = serde_json::json!({
            "__meta": { "version": "fixture" },
            "api": {
                "fetch": {
                    "__compat": {
                        "support": {
                            "nodejs": { "version_added": "18.0.0" },
                            "deno": { "version_added": "1.0" }
                        }
                    }
                }
            },
            "javascript": {
                "builtins": {
                    "Array": {
                        "toSorted": {
                            "__compat": {
                                "support": {
                                    "nodejs": { "version_added": "20.0.0" }
                                }
                            }
                        },
                        "options_parameter": {
                            "__compat": {
                                "support": {
                                    "nodejs": { "version_added": "21.0.0" }
                                }
                            }
                        },
                        "flagged": {
                            "__compat": {
                                "support": {
                                    "nodejs": {
                                        "version_added": "21.0.0",
                                        "flags": [{ "name": "flag" }]
                                    }
                                }
                            }
                        }
                    },
                    "Number": {
                        "MAX_SAFE_INTEGER": {
                            "__compat": {
                                "support": {
                                    "nodejs": { "version_added": "0.12.0" }
                                }
                            }
                        }
                    },
                    "Temporal": {
                        "__compat": {
                            "support": {
                                "nodejs": { "version_added": "26.0.0" }
                            }
                        }
                    }
                }
            }
        });

        let features = extract_features(&bcd, "nodejs").unwrap();
        let names: Vec<_> = features
            .iter()
            .map(|feature| feature.name.as_str())
            .collect();

        assert!(names.contains(&"fetch"));
        assert!(names.contains(&"Array.toSorted"));
        assert!(names.contains(&"Number.MAX_SAFE_INTEGER"));
        assert!(names.contains(&"Temporal"));
        assert!(!names.contains(&"Array.options_parameter"));
        assert!(!names.contains(&"Array.flagged"));

        let to_sorted = features
            .iter()
            .find(|feature| feature.name == "Array.toSorted")
            .unwrap();
        assert_eq!(to_sorted.version.to_string(), "20.0.0");
        assert!(
            to_sorted
                .detect
                .contains(&GeneratedDetectRule::Property("toSorted".to_string()))
        );
    }
}
