use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use node_semver::{Range, Version};
use serde_json::Value;

use crate::version::RuntimeVersion;

#[derive(Debug, Clone)]
pub struct EnginesReport {
    pub package_json: PathBuf,
    pub declared: Option<String>,
    pub required: RuntimeVersion,
    pub fixed: bool,
}

pub fn check_engines(
    root: &std::path::Path,
    required: RuntimeVersion,
    fix: bool,
) -> Result<Option<EnginesReport>> {
    let package_json = root.join("package.json");
    if !package_json.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&package_json)
        .with_context(|| format!("failed to read {}", package_json.display()))?;
    let mut json: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", package_json.display()))?;
    let declared = json
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let compatible = declared
        .as_deref()
        .is_some_and(|range| range_allows_required(range, required));
    let needs_fix = !required.is_zero() && !compatible;

    let mut fixed = false;
    if fix && needs_fix {
        set_engines_node(&mut json, &format!(">={required}"))?;
        fs::write(
            &package_json,
            format!("{}\n", serde_json::to_string_pretty(&json)?),
        )
        .with_context(|| format!("failed to write {}", package_json.display()))?;
        fixed = true;
    }

    if needs_fix || fixed {
        Ok(Some(EnginesReport {
            package_json,
            declared,
            required,
            fixed,
        }))
    } else {
        Ok(None)
    }
}

fn range_allows_required(range: &str, required: RuntimeVersion) -> bool {
    let Ok(range) = Range::parse(range) else {
        return false;
    };
    let Ok(version) = Version::parse(required.to_string()) else {
        return false;
    };
    range.satisfies(&version)
}

fn set_engines_node(json: &mut Value, value: &str) -> Result<()> {
    let root = json
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("package.json root must be an object"))?;
    let engines = root
        .entry("engines")
        .or_insert_with(|| Value::Object(Default::default()));
    let engines = engines
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("package.json engines must be an object"))?;
    engines.insert("node".to_owned(), Value::String(value.to_owned()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::range_allows_required;
    use crate::version::RuntimeVersion;

    #[test]
    fn checks_npm_style_ranges() {
        let required = RuntimeVersion {
            major: 24,
            minor: 0,
            patch: 0,
        };
        assert!(range_allows_required(">=22", required));
        assert!(!range_allows_required("^22.0.0", required));
    }
}
