#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from urllib.request import urlopen


DEFAULT_OUTPUT_DIR = Path("data/generated/mdn")
DEFAULT_RUNTIMES = ["nodejs", "deno", "bun", "safari", "chrome", "firefox"]
SYNTAX_SECTIONS = [
    "classes",
    "functions",
    "grammar",
    "operators",
    "regular_expressions",
    "statements",
]
IGNORED_RUNTIME_FEATURES = {
    "undefined",
}


@dataclass(frozen=True, order=True)
class Version:
    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, value: str) -> Version:
        value = value.strip().removeprefix("v")
        if not value or not value[0].isdigit():
            raise ValueError(f"version is not numeric: {value}")

        parts = value.split(".")
        while len(parts) < 3:
            parts.append("0")
        return cls(*(int(part) for part in parts[:3]))

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}"


@dataclass(frozen=True)
class RuntimeTarget:
    bcd_id: str
    runtime_name: str


def main() -> int:
    args = parse_args()
    bcd_version = args.bcd_version or Path("data/mdn-bcd.version").read_text().strip()
    bcd = load_bcd(args.input, bcd_version)

    actual = bcd.get("__meta", {}).get("version")
    if actual and actual != bcd_version:
        raise SystemExit(f"expected BCD {bcd_version}, got {actual}")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    for runtime_id in args.runtimes:
        target = runtime_target(runtime_id)
        features = extract_features(bcd, target.bcd_id)
        output = {
            "schema": 1,
            "runtime": target.runtime_name,
            "source": {
                "name": "@mdn/browser-compat-data",
                "version": bcd_version,
                "runtime": target.bcd_id,
            },
            "features": features,
        }
        path = args.output_dir / f"{target.runtime_name}.json"
        path.write_text(json.dumps(output, indent=2, sort_keys=False) + "\n", encoding="utf-8")
        print(
            f"generated {len(features)} features for {target.runtime_name} "
            f"from MDN BCD {bcd_version} -> {path}"
        )

    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="generate_mdn_data.py",
        description="Generate JSON runtime data artifacts from MDN browser-compat-data.",
    )
    parser.add_argument("--input", type=Path, help="read MDN BCD data.json from disk")
    parser.add_argument(
        "--bcd-version",
        help="MDN @mdn/browser-compat-data version to download",
    )
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help=f"directory where JSON artifacts are written (default: {DEFAULT_OUTPUT_DIR})",
    )
    parser.add_argument(
        "--runtimes",
        default=",".join(DEFAULT_RUNTIMES),
        help="comma-separated BCD runtime ids: nodejs,deno,bun,safari,chrome,firefox",
    )
    args = parser.parse_args()
    args.runtimes = [runtime.strip() for runtime in args.runtimes.split(",") if runtime.strip()]
    return args


def load_bcd(input_path: Path | None, version: str) -> dict:
    if input_path:
        return json.loads(input_path.read_text(encoding="utf-8"))

    url = f"https://unpkg.com/@mdn/browser-compat-data@{version}/data.json"
    with urlopen(url) as response:
        return json.loads(response.read().decode("utf-8"))


def runtime_target(runtime_id: str) -> RuntimeTarget:
    match runtime_id:
        case "nodejs" | "node":
            return RuntimeTarget("nodejs", "node")
        case "deno":
            return RuntimeTarget("deno", "deno")
        case "bun":
            return RuntimeTarget("bun", "bun")
        case "safari":
            return RuntimeTarget("safari", "safari")
        case "chrome":
            return RuntimeTarget("chrome", "chrome")
        case "firefox":
            return RuntimeTarget("firefox", "firefox")
        case _:
            raise SystemExit(f"unsupported BCD runtime id `{runtime_id}`")


def extract_features(bcd: dict, runtime_id: str) -> list[dict]:
    features: dict[str, dict] = {}

    api = bcd.get("api")
    if isinstance(api, dict):
        walk_compat_tree(api, [], "api", runtime_id, features)

    builtins = bcd.get("javascript", {}).get("builtins")
    if isinstance(builtins, dict):
        walk_compat_tree(builtins, [], "builtin", runtime_id, features)

    javascript = bcd.get("javascript", {})
    for section in SYNTAX_SECTIONS:
        syntax = javascript.get(section)
        if isinstance(syntax, dict):
            walk_compat_tree(syntax, [], f"syntax:{section}", runtime_id, features)

    return [features[name] for name in sorted(features)]


def walk_compat_tree(
    value: dict,
    path: list[str],
    source: str,
    runtime_id: str,
    features: dict[str, dict],
) -> None:
    compat = value.get("__compat")
    if isinstance(compat, dict):
        version = support_version(compat, runtime_id)
        name = feature_name(source, path)
        if version and name and is_supported_path(source, path):
            detect = detect_rules(source, path)
            if detect:
                upsert_feature(
                    features,
                    {
                        "name": name,
                        "version": str(version),
                        "detect": detect,
                    },
                )

    for key, child in value.items():
        if key == "__compat" or not isinstance(child, dict):
            continue
        path.append(key)
        walk_compat_tree(child, path, source, runtime_id, features)
        path.pop()


def upsert_feature(features: dict[str, dict], feature: dict) -> None:
    existing = features.get(feature["name"])
    if existing and Version.parse(existing["version"]) <= Version.parse(feature["version"]):
        return
    features[feature["name"]] = feature


def support_version(compat: dict, runtime_id: str) -> Version | None:
    support = compat.get("support", {}).get(runtime_id)
    if support is None:
        return None

    statements = support if isinstance(support, list) else [support]
    versions = []
    for statement in statements:
        if not isinstance(statement, dict):
            continue
        if statement.get("flags"):
            continue
        if statement.get("version_removed") is not None:
            continue
        version_added = statement.get("version_added")
        if not isinstance(version_added, str):
            continue
        try:
            versions.append(Version.parse(version_added))
        except ValueError:
            continue
    return min(versions) if versions else None


def feature_name(source: str, path: list[str]) -> str:
    name = ".".join(path)
    if source.startswith("syntax:"):
        section = source.split(":", 1)[1]
        return f"syntax.{section}.{name}" if name else ""
    return name


def is_supported_path(source: str, path: list[str]) -> bool:
    if source.startswith("syntax:"):
        return bool(path) and all(is_syntax_segment(segment) for segment in path)
    return is_runtime_surface_path(path)


def is_runtime_surface_path(path: list[str]) -> bool:
    if not path:
        return False
    if ".".join(path) in IGNORED_RUNTIME_FEATURES:
        return False
    if len(path) > 1 and path[0] == path[-1]:
        return False
    return all(is_runtime_surface_segment(segment) for segment in path)


def is_runtime_surface_segment(segment: str) -> bool:
    if not segment or segment.startswith("@@") or "-" in segment:
        return False
    if "_" in segment:
        return all(ch == "_" or ch.isdigit() or ("A" <= ch <= "Z") for ch in segment)
    return all(ch == "$" or ch.isalnum() for ch in segment)


def is_syntax_segment(segment: str) -> bool:
    return bool(segment) and all(ch == "_" or ch.isalnum() for ch in segment)


def detect_rules(source: str, path: list[str]) -> list[dict]:
    if not path:
        return []

    name = ".".join(path)
    rules = set()
    if source == "api" or source == "builtin":
        if len(path) == 1:
            rules.add(("Global", name))
        rules.add(("MemberChain", name))
        if source == "builtin" and len(path) >= 2 and is_detectable_property(path[-1]):
            rules.add(("Property", path[-1]))
    elif source.startswith("syntax:"):
        section = source.split(":", 1)[1]
        rules.add(("Syntax", f"{section}.{name}"))

    return [{"kind": kind, "value": value} for kind, value in sorted(rules)]


def is_detectable_property(property_name: str) -> bool:
    return (
        not property_name.startswith("@@")
        and not property_name.startswith("__")
        and bool(property_name)
        and (property_name[0] == "_" or property_name[0].isalpha())
    )


if __name__ == "__main__":
    sys.exit(main())
