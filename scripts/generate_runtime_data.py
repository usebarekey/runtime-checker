#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path


RUNTIMES = ["node", "deno", "bun", "safari", "chrome", "firefox"]
RON_SOURCES = {
    "node": Path("data/node.ron"),
    "deno": Path("data/mdn/deno.ron"),
    "bun": Path("data/mdn/bun.ron"),
    "safari": Path("data/mdn/safari.ron"),
    "chrome": Path("data/mdn/chrome.ron"),
    "firefox": Path("data/mdn/firefox.ron"),
}
JSON_SOURCES = {
    runtime: Path("data/generated/mdn") / f"{runtime}.json" for runtime in RUNTIMES
}
CANONICAL_MEMBER_PREFIXES = {
    "database.": "sqlite.DatabaseSync.",
    "statement.": "sqlite.StatementSync.",
    "sqlTagStore.": "sqlite.SQLTagStore.",
}
IGNORED_FEATURES = {
    "undefined",
}
GENERIC_PROPERTY_OWNERS = {
    "Array",
    "String",
    "TypedArray",
}
DEFAULT_OUTPUT = Path("src/generated/runtime_data.rs")
ROW_RE = re.compile(
    r'\(name:\s*(?P<name>"(?:\\.|[^"\\])*")'
    r',\s*version:\s*(?P<version>"(?:\\.|[^"\\])*")'
    r',\s*detect:\s*\[(?P<detect>.*?)\]\),'
)
DETECT_RE = re.compile(
    r'(?P<kind>Global|MemberChain|Property|Syntax|Support)\('
    r'(?P<value>"(?:\\.|[^"\\])*")'
    r'\)'
)


@dataclass(frozen=True, order=True)
class Version:
    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, value: str) -> Version:
        parts = value.removeprefix("v").split(".")
        while len(parts) < 3:
            parts.append("0")
        return cls(*(int(part) for part in parts[:3]))


@dataclass(frozen=True)
class Feature:
    name: str
    version: Version
    detect: tuple[tuple[str, str], ...]


def main() -> int:
    args = parse_args()
    runtimes = [runtime.strip() for runtime in args.runtimes.split(",") if runtime.strip()]
    unknown = sorted(set(runtimes) - set(RUNTIMES))
    if unknown:
        raise SystemExit(f"unknown runtimes: {', '.join(unknown)}")

    output = render_module({runtime: load_runtime(runtime) for runtime in runtimes})
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(output, encoding="utf-8")
    print(f"generated static runtime data -> {args.output}")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="generate_runtime_data.py",
        description="Generate Rust static runtime lookup tables.",
    )
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--runtimes", default=",".join(RUNTIMES))
    return parser.parse_args()


def load_runtime(runtime: str) -> list[Feature]:
    by_name: dict[str, Feature] = {}
    for feature in read_ron_features(RON_SOURCES[runtime]):
        upsert_feature(by_name, feature)

    generated_json = JSON_SOURCES[runtime]
    if generated_json.exists():
        for feature in read_json_features(generated_json):
            upsert_feature(by_name, feature)

    return [by_name[name] for name in sorted(by_name)]


def upsert_feature(features: dict[str, Feature], feature: Feature) -> None:
    feature = canonicalize_feature(feature)
    if feature.name in IGNORED_FEATURES:
        return
    existing = features.get(feature.name)
    if existing:
        version = existing.version if existing.version <= feature.version else feature.version
        detect = tuple(sorted(set(existing.detect) | set(feature.detect)))
        features[feature.name] = Feature(feature.name, version, detect)
        return
    features[feature.name] = feature


def canonicalize_feature(feature: Feature) -> Feature:
    for alias_prefix, canonical_prefix in CANONICAL_MEMBER_PREFIXES.items():
        if feature.name.startswith(alias_prefix):
            canonical_name = canonical_prefix + feature.name.removeprefix(alias_prefix)
            detect = set(feature.detect)
            detect.add(("MemberChain", feature.name))
            detect.add(("MemberChain", canonical_name))
            return Feature(canonical_name, feature.version, tuple(sorted(detect)))
    return feature


def read_ron_features(path: Path) -> list[Feature]:
    text = path.read_text(encoding="utf-8")
    features = []
    for matched in ROW_RE.finditer(text):
        name = json.loads(matched.group("name"))
        version = Version.parse(json.loads(matched.group("version")))
        detect = tuple(
            (detect.group("kind"), json.loads(detect.group("value")))
            for detect in DETECT_RE.finditer(matched.group("detect"))
        )
        if detect:
            features.append(Feature(name, version, detect))
    return features


def read_json_features(path: Path) -> list[Feature]:
    data = json.loads(path.read_text(encoding="utf-8"))
    features = []
    for row in data["features"]:
        detect = tuple((detect["kind"], detect["value"]) for detect in row["detect"])
        if detect:
            features.append(Feature(row["name"], Version.parse(row["version"]), detect))
    return features


def render_module(runtimes: dict[str, list[Feature]]) -> str:
    lines = [
        "// This file is @generated by scripts/generate_runtime_data.py.",
        "// Do not edit by hand.",
        "",
        "use crate::{",
        "    data::{Feature, RuntimeDb},",
        "    version::RuntimeVersion,",
        "};",
        "",
    ]

    for runtime, features in runtimes.items():
        upper = runtime.upper()
        feature_name_to_index = {feature.name: index for index, feature in enumerate(features)}
        lookup = build_lookup_tables(features, feature_name_to_index)

        lines.extend(render_features(upper, features))
        lines.extend(render_lookup_map(upper, "GLOBALS", lookup["Global"]))
        lines.extend(render_lookup_map(upper, "MEMBER_CHAINS", lookup["MemberChain"]))
        lines.extend(render_lookup_map(upper, "PROPERTIES", lookup["Property"]))
        lines.extend(render_lookup_map(upper, "SYNTAX", lookup["Syntax"]))
        lines.extend(render_lookup_map(upper, "SUPPORT", lookup["Support"]))
        lines.extend(render_fast_patterns(upper, lookup))
        lines.extend(
            [
                f"pub static {upper}: RuntimeDb = RuntimeDb {{",
                f"    name: {rust_string(runtime)},",
                f"    features: {upper}_FEATURES,",
                f"    globals: &{upper}_GLOBALS,",
                f"    member_chains: &{upper}_MEMBER_CHAINS,",
                f"    properties: &{upper}_PROPERTIES,",
                f"    syntax: &{upper}_SYNTAX,",
                f"    support: &{upper}_SUPPORT,",
                f"    fast_patterns: {upper}_FAST_PATTERNS,",
                "};",
                "",
            ]
        )

    return "\n".join(lines)


def build_lookup_tables(
    features: list[Feature], feature_name_to_index: dict[str, int]
) -> dict[str, list[tuple[str, int]]]:
    tables: dict[str, dict[str, int]] = {
        "Global": {},
        "MemberChain": {},
        "Property": {},
        "Syntax": {},
        "Support": {},
    }

    property_keys = safe_property_keys(features)

    for feature in features:
        candidate = feature_name_to_index[feature.name]
        for kind, key in feature.detect:
            if kind == "Property" and key not in property_keys:
                continue
            table = tables[kind]
            existing = table.get(key)
            if existing is None or features[existing].version < feature.version:
                table[key] = candidate

    return {kind: sorted(table.items()) for kind, table in tables.items()}


def safe_property_keys(features: list[Feature]) -> set[str]:
    candidates: dict[str, set[tuple[str, Version]]] = {}
    for feature in features:
        owner = feature.name.split(".", 1)[0]
        for kind, key in feature.detect:
            if kind == "Property":
                candidates.setdefault(key, set()).add((owner, feature.version))

    return {
        key
        for key, owners_and_versions in candidates.items()
        if all(owner in GENERIC_PROPERTY_OWNERS for owner, _version in owners_and_versions)
        and len({version for _owner, version in owners_and_versions}) == 1
    }


def render_features(upper: str, features: list[Feature]) -> list[str]:
    lines = [f"static {upper}_FEATURES: &[Feature] = &["]
    for index, feature in enumerate(features):
        lines.extend(
            [
                "    Feature {",
                f"        id: {index},",
                f"        name: {rust_string(feature.name)},",
                "        version: RuntimeVersion {",
                f"            major: {feature.version.major},",
                f"            minor: {feature.version.minor},",
                f"            patch: {feature.version.patch},",
                "        },",
                "    },",
            ]
        )
    lines.extend(["];", ""])
    return lines


def render_lookup_map(upper: str, table: str, rows: list[tuple[str, int]]) -> list[str]:
    lines = [
        f"static {upper}_{table}: phf::Map<&'static str, usize> = phf::phf_map! {{"
    ]
    for key, feature_index in rows:
        lines.append(f"    {rust_string(key)} => {feature_index},")
    lines.extend(["};", ""])
    return lines


def render_fast_patterns(
    upper: str, lookup: dict[str, list[tuple[str, int]]]
) -> list[str]:
    patterns = sorted(
        {
            key
            for kind in ["Global", "MemberChain", "Property"]
            for key, _feature_index in lookup[kind]
        },
        key=lambda value: (-len(value), value),
    )
    lines = [f"static {upper}_FAST_PATTERNS: &[&str] = &["]
    lines.extend(f"    {rust_string(pattern)}," for pattern in patterns)
    lines.extend(["];", ""])
    return lines


def rust_string(value: str) -> str:
    return json.dumps(value)


if __name__ == "__main__":
    sys.exit(main())
