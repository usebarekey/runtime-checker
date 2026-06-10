use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RuntimeSpec {
    schema: u32,
    runtime: String,
    features: Vec<FeatureSpec>,
}

#[derive(Debug, Deserialize)]
struct FeatureSpec {
    name: String,
    version: String,
    detect: Vec<DetectRule>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
enum DetectRule {
    Global(String),
    MemberChain(String),
    Property(String),
}

#[test]
fn generated_mdn_runtime_data_is_valid_ron() {
    let node = load("data/mdn/node.ron");
    let deno = load("data/mdn/deno.ron");
    let bun = load("data/mdn/bun.ron");
    let safari = load("data/mdn/safari.ron");
    let chrome = load("data/mdn/chrome.ron");
    let firefox = load("data/mdn/firefox.ron");

    assert_eq!(node.runtime, "node");
    assert_eq!(deno.runtime, "deno");
    assert_eq!(bun.runtime, "bun");
    assert_eq!(safari.runtime, "safari");
    assert_eq!(chrome.runtime, "chrome");
    assert_eq!(firefox.runtime, "firefox");

    assert_feature(&node, "Temporal", "26.0.0");
    assert_feature(&node, "Array.toSorted", "20.0.0");
    assert_feature(&deno, "Temporal", "2.7.0");
    assert_feature(&bun, "fetch", "1.0.0");
    assert_feature(&safari, "Array.toSorted", "16.0.0");
    assert_feature(&chrome, "Array.toSorted", "110.0.0");
    assert_feature(&firefox, "Temporal", "139.0.0");
}

fn load(path: &str) -> RuntimeSpec {
    let text = std::fs::read_to_string(path).unwrap();
    let spec: RuntimeSpec = ron::from_str(&text).unwrap();
    assert_eq!(spec.schema, 1);
    assert!(!spec.features.is_empty());
    assert!(
        spec.features
            .iter()
            .all(|feature| !feature.detect.is_empty())
    );
    spec
}

fn assert_feature(spec: &RuntimeSpec, name: &str, version: &str) {
    let feature = spec
        .features
        .iter()
        .find(|feature| feature.name == name)
        .unwrap_or_else(|| panic!("missing feature {name}"));

    assert_eq!(feature.version, version);
}
