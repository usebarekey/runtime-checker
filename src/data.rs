use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{cli::RuntimeKind, version::RuntimeVersion};

static NODE_SPEC: &str = include_str!("../data/node.ron");
static DENO_SPEC: &str = include_str!("../data/mdn/deno.ron");
static BUN_SPEC: &str = include_str!("../data/mdn/bun.ron");
static SAFARI_SPEC: &str = include_str!("../data/mdn/safari.ron");
static CHROME_SPEC: &str = include_str!("../data/mdn/chrome.ron");
static FIREFOX_SPEC: &str = include_str!("../data/mdn/firefox.ron");
static NODE_RUNTIME: OnceLock<RuntimeDb> = OnceLock::new();
static DENO_RUNTIME: OnceLock<RuntimeDb> = OnceLock::new();
static BUN_RUNTIME: OnceLock<RuntimeDb> = OnceLock::new();
static SAFARI_RUNTIME: OnceLock<RuntimeDb> = OnceLock::new();
static CHROME_RUNTIME: OnceLock<RuntimeDb> = OnceLock::new();
static FIREFOX_RUNTIME: OnceLock<RuntimeDb> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct RuntimeSpec {
    schema: u32,
    runtime: String,
    features: Vec<FeatureSpec>,
}

#[derive(Debug, Deserialize)]
struct FeatureSpec {
    name: String,
    version: RuntimeVersion,
    detect: Vec<DetectRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum DetectRule {
    Global(String),
    MemberChain(String),
    Property(String),
}

#[derive(Debug)]
pub struct RuntimeDb {
    name: String,
    features: Vec<Feature>,
    globals: HashMap<String, usize>,
    member_chains: HashMap<String, usize>,
    properties: HashMap<String, usize>,
    fast_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub id: usize,
    pub name: String,
    pub version: RuntimeVersion,
}

impl RuntimeDb {
    fn from_spec(spec: RuntimeSpec) -> Result<Self> {
        anyhow::ensure!(
            spec.schema == 1,
            "unsupported {} schema {}",
            spec.runtime,
            spec.schema
        );

        let mut db = Self {
            name: spec.runtime,
            features: Vec::with_capacity(spec.features.len()),
            globals: HashMap::new(),
            member_chains: HashMap::new(),
            properties: HashMap::new(),
            fast_patterns: Vec::new(),
        };

        let mut patterns = HashSet::new();
        for feature in spec.features {
            let index = db.features.len();
            db.features.push(Feature {
                id: index,
                name: feature.name,
                version: feature.version,
            });

            for rule in feature.detect {
                match rule {
                    DetectRule::Global(name) => {
                        db.insert_highest_global(name.clone(), index);
                        patterns.insert(name);
                    }
                    DetectRule::MemberChain(name) => {
                        db.insert_highest_member_chain(name.clone(), index);
                        patterns.insert(name);
                    }
                    DetectRule::Property(name) => {
                        db.insert_highest_property(name.clone(), index);
                        patterns.insert(name);
                    }
                }
            }
        }

        db.fast_patterns = patterns.into_iter().collect();
        db.fast_patterns
            .sort_by_key(|pattern| std::cmp::Reverse(pattern.len()));
        Ok(db)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fast_patterns(&self) -> &[String] {
        &self.fast_patterns
    }

    pub fn feature_for_pattern(&self, pattern: &str) -> Option<&Feature> {
        self.globals
            .get(pattern)
            .or_else(|| self.member_chains.get(pattern))
            .or_else(|| self.properties.get(pattern))
            .and_then(|index| self.features.get(*index))
    }

    pub fn is_global_or_member_pattern(&self, pattern: &str) -> bool {
        self.globals.contains_key(pattern) || self.member_chains.contains_key(pattern)
    }

    pub fn is_property_pattern(&self, pattern: &str) -> bool {
        self.properties.contains_key(pattern)
    }

    pub fn match_global(&self, name: &str) -> Option<&Feature> {
        self.globals
            .get(name)
            .and_then(|index| self.features.get(*index))
    }

    pub fn match_member_chain(&self, name: &str) -> Option<&Feature> {
        self.member_chains
            .get(name)
            .and_then(|index| self.features.get(*index))
    }

    pub fn match_property(&self, name: &str) -> Option<&Feature> {
        self.properties
            .get(name)
            .and_then(|index| self.features.get(*index))
    }

    fn insert_highest_global(&mut self, name: String, index: usize) {
        insert_highest(&self.features, &mut self.globals, name, index);
    }

    fn insert_highest_member_chain(&mut self, name: String, index: usize) {
        insert_highest(&self.features, &mut self.member_chains, name, index);
    }

    fn insert_highest_property(&mut self, name: String, index: usize) {
        insert_highest(&self.features, &mut self.properties, name, index);
    }
}

fn insert_highest(
    features: &[Feature],
    map: &mut HashMap<String, usize>,
    key: String,
    candidate: usize,
) {
    match map.get(&key).copied() {
        Some(existing) if features[existing].version >= features[candidate].version => {}
        _ => {
            map.insert(key, candidate);
        }
    }
}

#[cfg(test)]
pub fn node_runtime() -> Result<&'static RuntimeDb> {
    runtime(RuntimeKind::Node)
}

pub fn runtime(kind: RuntimeKind) -> Result<&'static RuntimeDb> {
    match kind {
        RuntimeKind::All => anyhow::bail!("all is not a concrete runtime database"),
        RuntimeKind::Node => load_runtime(&NODE_RUNTIME, NODE_SPEC, "data/node.ron"),
        RuntimeKind::Deno => load_runtime(&DENO_RUNTIME, DENO_SPEC, "data/mdn/deno.ron"),
        RuntimeKind::Bun => load_runtime(&BUN_RUNTIME, BUN_SPEC, "data/mdn/bun.ron"),
        RuntimeKind::Safari => load_runtime(&SAFARI_RUNTIME, SAFARI_SPEC, "data/mdn/safari.ron"),
        RuntimeKind::Chrome => load_runtime(&CHROME_RUNTIME, CHROME_SPEC, "data/mdn/chrome.ron"),
        RuntimeKind::Firefox => {
            load_runtime(&FIREFOX_RUNTIME, FIREFOX_SPEC, "data/mdn/firefox.ron")
        }
    }
}

fn load_runtime(
    lock: &'static OnceLock<RuntimeDb>,
    source: &'static str,
    source_name: &str,
) -> Result<&'static RuntimeDb> {
    if let Some(runtime) = lock.get() {
        return Ok(runtime);
    }

    let spec = ron::from_str::<RuntimeSpec>(source)
        .with_context(|| format!("failed to parse {source_name}"))?;
    let runtime = RuntimeDb::from_spec(spec)?;
    Ok(lock.get_or_init(|| runtime))
}

#[cfg(test)]
mod tests {
    use crate::cli::RuntimeKind;

    use super::{node_runtime, runtime};

    #[test]
    fn loads_node_ron_database() {
        let db = node_runtime().unwrap();
        assert_eq!(db.name(), "node");
        assert_eq!(
            db.match_global("Temporal").unwrap().version.to_string(),
            "26.0.0"
        );
        assert_eq!(
            db.match_property("toSorted").unwrap().version.to_string(),
            "20.0.0"
        );
    }

    #[test]
    fn loads_mdn_runtime_databases() {
        let deno = runtime(RuntimeKind::Deno).unwrap();
        let bun = runtime(RuntimeKind::Bun).unwrap();
        let safari = runtime(RuntimeKind::Safari).unwrap();
        let chrome = runtime(RuntimeKind::Chrome).unwrap();
        let firefox = runtime(RuntimeKind::Firefox).unwrap();

        assert_eq!(deno.name(), "deno");
        assert_eq!(bun.name(), "bun");
        assert_eq!(safari.name(), "safari");
        assert_eq!(chrome.name(), "chrome");
        assert_eq!(firefox.name(), "firefox");
        assert_eq!(
            deno.match_global("Temporal").unwrap().version.to_string(),
            "2.7.0"
        );
        assert_eq!(
            bun.match_global("fetch").unwrap().version.to_string(),
            "1.0.0"
        );
        assert_eq!(
            chrome
                .match_property("toSorted")
                .unwrap()
                .version
                .to_string(),
            "110.0.0"
        );
    }
}
