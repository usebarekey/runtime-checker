use anyhow::Result;

use crate::{cli::RuntimeKind, generated::runtime_data, version::RuntimeVersion};

#[derive(Debug)]
pub struct RuntimeDb {
    pub name: &'static str,
    pub features: &'static [Feature],
    pub globals: &'static phf::Map<&'static str, usize>,
    pub member_chains: &'static phf::Map<&'static str, usize>,
    pub properties: &'static phf::Map<&'static str, usize>,
    pub syntax: &'static phf::Map<&'static str, usize>,
    pub support: &'static phf::Map<&'static str, usize>,
    pub fast_patterns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct Feature {
    pub id: usize,
    pub name: &'static str,
    pub version: RuntimeVersion,
}

impl RuntimeDb {
    pub fn name(&self) -> &str {
        self.name
    }

    pub fn fast_patterns(&self) -> &[&'static str] {
        self.fast_patterns
    }

    pub fn feature_for_pattern(&self, pattern: &str) -> Option<&Feature> {
        self.match_global(pattern)
            .or_else(|| self.match_member_chain(pattern))
            .or_else(|| self.match_property(pattern))
    }

    pub fn is_global_or_member_pattern(&self, pattern: &str) -> bool {
        self.globals.contains_key(pattern) || self.member_chains.contains_key(pattern)
    }

    pub fn is_property_pattern(&self, pattern: &str) -> bool {
        self.properties.contains_key(pattern)
    }

    pub fn match_global(&self, name: &str) -> Option<&Feature> {
        self.lookup(self.globals, name)
    }

    pub fn match_member_chain(&self, name: &str) -> Option<&Feature> {
        self.lookup(self.member_chains, name)
    }

    pub fn match_property(&self, name: &str) -> Option<&Feature> {
        self.lookup(self.properties, name)
    }

    #[allow(dead_code)]
    pub fn match_syntax(&self, name: &str) -> Option<&Feature> {
        self.lookup(self.syntax, name)
    }

    pub fn match_support(&self, name: &str) -> Option<&Feature> {
        self.lookup(self.support, name)
    }

    fn lookup(&self, map: &'static phf::Map<&'static str, usize>, key: &str) -> Option<&Feature> {
        map.get(key).and_then(|index| self.features.get(*index))
    }
}

#[cfg(test)]
pub fn node_runtime() -> Result<&'static RuntimeDb> {
    runtime(RuntimeKind::Node)
}

pub fn runtime(kind: RuntimeKind) -> Result<&'static RuntimeDb> {
    match kind {
        RuntimeKind::All => anyhow::bail!("all is not a concrete runtime database"),
        RuntimeKind::Node => Ok(&runtime_data::NODE),
        RuntimeKind::Deno => Ok(&runtime_data::DENO),
        RuntimeKind::Bun => Ok(&runtime_data::BUN),
        RuntimeKind::Safari => Ok(&runtime_data::SAFARI),
        RuntimeKind::Chrome => Ok(&runtime_data::CHROME),
        RuntimeKind::Firefox => Ok(&runtime_data::FIREFOX),
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::RuntimeKind;

    use super::{node_runtime, runtime};

    #[test]
    fn loads_node_static_database() {
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
        assert_eq!(
            db.match_syntax("operators.logical_or_assignment")
                .unwrap()
                .version
                .to_string(),
            "15.0.0"
        );
        assert!(db.match_global("undefined").is_none());
        assert!(db.match_member_chain("undefined").is_none());
        assert_eq!(
            db.match_support("module.esm").unwrap().version.to_string(),
            "13.2.0"
        );
        assert_eq!(
            db.match_support("typescript.native")
                .unwrap()
                .version
                .to_string(),
            "22.6.0"
        );
    }

    #[test]
    fn loads_mdn_static_databases() {
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
        assert_eq!(
            chrome
                .match_syntax("operators.logical_or_assignment")
                .unwrap()
                .version
                .to_string(),
            "85.0.0"
        );
        assert!(deno.match_global("undefined").is_none());
        assert!(bun.match_global("undefined").is_none());
        assert!(safari.match_global("undefined").is_none());
        assert!(chrome.match_global("undefined").is_none());
        assert!(firefox.match_global("undefined").is_none());
    }
}
