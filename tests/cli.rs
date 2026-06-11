use assert_cmd::Command;
use std::fs;
use tempfile::tempdir;

fn command() -> Command {
    let mut command = Command::cargo_bin("runtime-checker").unwrap();
    command.env("CARGO_TERM_COLOR", "always");
    command
}

fn visible_text(value: &str) -> String {
    let mut output = String::new();
    let mut in_escape = false;

    for ch in value.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }

        if ch == '\x1b' {
            in_escape = true;
            continue;
        }

        output.push(ch);
    }

    output
}

#[test]
fn default_mode_reports_detected_minimum() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("date.ts"),
        "Temporal.Now.instant();\nconst values = [3, 1, 2].toSorted();\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Temporal"));
    assert!(visible.contains("Array.toSorted"));
    assert!(visible.contains("Runtimes"));
    assert!(visible.contains("Browsers"));
    assert!(visible.contains("Node.js 26.0.0"));
    assert!(visible.contains("Deno 2.7.0"));
    assert!(visible.contains("Bun 1.0.0"));
    assert!(visible.contains("Safari 16.0.0"));
    assert!(visible.contains("Chromium 144.0.0"));
    assert!(visible.contains("Firefox 139.0.0"));
    assert!(visible.contains("Finished in "));
    assert!(visible.contains(" using oxc (ast parsing) after scanning "));
    assert!(visible.contains("after scanning "));
    assert!(visible.contains("26.0.0"));
}

#[test]
fn summary_prints_only_result_panel() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("date.ts"),
        "Temporal.Now.instant();\nTemporal.Now.instant();\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(!visible.contains("Temporal • v"));
    assert!(visible.contains("runtime-checker"));
    assert!(visible.contains("Finished in"));
    assert!(visible.contains("using oxc (ast parsing)"));
    assert!(visible.contains("scanning "));
    assert!(visible.contains("2 lines of code"));
    assert!(visible.contains("Runtimes"));
    assert!(visible.contains("Browsers"));
    assert!(visible.contains("Node.js 26.0.0"));
    assert!(visible.contains("Deno 2.7.0"));
    assert!(visible.contains("Bun 0.0.0"));
    assert!(visible.contains("Safari 0.0.0"));
    assert!(visible.contains("Chromium 144.0.0"));
    assert!(visible.contains("Firefox 139.0.0"));
    assert!(visible.contains("26.0.0"));
    assert!(!visible.contains("[⚡]"));
    assert!(!visible.contains("[◆]"));
    assert!(!visible.contains("done"));
}

#[test]
fn help_uses_barekey_style_sections() {
    let output = command()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("─ runtime-checker  0.1.0"));
    assert!(visible.contains("Usage »"));
    assert!(visible.contains("Arguments »"));
    assert!(visible.contains("Options »"));
    assert!(visible.contains("runtime-checker <dir> [options]"));
    assert!(visible.contains("--summary, --oneline"));
    assert!(visible.contains("-h, --help"));
}

#[test]
fn no_args_prints_help() {
    let output = command().assert().success().get_output().stdout.clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Usage »"));
    assert!(visible.contains("runtime-checker <dir> [options]"));
}

#[test]
fn browser_summary_marks_node_api_incompatibility() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("fs.ts"),
        "import * as fs from 'node:fs';\nfs.cp('a', 'b', () => {});\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Browsers"));
    assert!(visible.contains("Safari¹"));
    assert!(visible.contains("Chromium²"));
    assert!(visible.contains("Firefox³"));
    assert!(visible.contains("¹ Safari does not support Node APIs."));
    assert!(visible.contains("² Chromium does not support Node APIs."));
    assert!(visible.contains("³ Firefox does not support Node APIs."));
}

#[test]
fn browser_only_summary_marks_node_api_incompatibility() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("process.ts"), "process.cwd();\n").unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("chrome")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Chromium¹"));
    assert!(visible.contains("¹ Chromium does not support Node APIs."));
}

#[test]
fn groups_repeated_features_and_inspects_all_hits() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("json.ts"),
        "JSON.stringify({ one: 1 });\nJSON.stringify({ two: 2 });\n",
    )
    .unwrap();

    let grouped = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let grouped = String::from_utf8_lossy(&grouped);
    let grouped = visible_text(&grouped);
    assert!(grouped.contains("JSON.stringify"));
    assert!(grouped.contains("(x2)"));
    assert_eq!(grouped.matches("JSON.stringify").count(), 1);

    let inspected = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--inspect")
        .arg("JSON.stringify")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let inspected = String::from_utf8_lossy(&inspected);
    let inspected = visible_text(&inspected);
    assert_eq!(inspected.matches("JSON.stringify").count(), 2);
    assert!(inspected.contains("json.ts@1:1"));
    assert!(inspected.contains("json.ts@2:1"));
}

#[test]
fn group_header_uses_lowest_detected_version_in_major() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("timers.ts"),
        "setTimeout(() => {}, 1);\nprocess.cwd();\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Node.js 0.0.1"));
    assert!(!visible.contains("Node.js 0.0.0"));
}

#[test]
fn fast_mode_counts_text_matches() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("runtime.ts"), "Temporal.Now.instant();\n").unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--fast")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Temporal"));
    assert!(visible.contains("26.0.0"));
    assert!(visible.contains("using fff (text scan)"));
}

#[test]
fn fast_mode_counts_comments_and_strings() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("comment.ts"),
        "// Temporal in a comment counts in FFF mode\nconst text = \"Temporal\";\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--fast")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Node.js 26.0.0"));
}

#[test]
fn fast_mode_uses_same_ignored_file_scope_as_ast_mode() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".vendors")).unwrap();
    fs::write(
        dir.path().join(".vendors").join("vendored.ts"),
        "Temporal.Now.instant();\n",
    )
    .unwrap();
    fs::write(dir.path().join("app.ts"), "const ok = 1;\n").unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--fast")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Node.js 0.0.0"));
    assert!(!visible.contains("26.0.0"));
}

#[test]
fn fast_mode_ignores_unsafe_bare_lowercase_tokens() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("app.ts"),
        "import { value } from './value';\nfunction run(params: { text: string }) { const array = [3, 1, 2].toSorted(); return array; }\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--fast")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Node.js 20.0.0"));
    assert!(!visible.contains("25.9.0"));
}

#[test]
fn fast_mode_prefers_longest_overlapping_feature_token() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("iter.ts"),
        "GRID_RANGE.flatMap((row) => row);\n",
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--fast")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);

    assert!(visible.contains("Node.js 22.0.0"));
}

#[test]
fn supports_non_node_runtime_targets() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("runtime.ts"),
        "fetch('/api');\nconst values = [3, 1, 2].toSorted();\nTemporal.Now.instant();\n",
    )
    .unwrap();

    let deno = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("deno")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let deno = visible_text(&String::from_utf8_lossy(&deno));
    assert!(deno.contains("Deno 2.7.0"));

    let bun = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("bun")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let bun = visible_text(&String::from_utf8_lossy(&bun));
    assert!(bun.contains("Bun 1.0.0"));

    let chrome = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("chrome")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let chrome = visible_text(&String::from_utf8_lossy(&chrome));
    assert!(chrome.contains("Chromium 144.0.0"));

    let safari = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("safari")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let safari = visible_text(&String::from_utf8_lossy(&safari));
    assert!(safari.contains("Safari 16.0.0"));

    let firefox = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("firefox")
        .arg("--summary")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let firefox = visible_text(&String::from_utf8_lossy(&firefox));
    assert!(firefox.contains("Firefox 139.0.0"));
}

#[test]
fn fix_is_node_only() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("runtime.ts"), "fetch('/api');\n").unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("deno")
        .arg("--fix")
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let output = String::from_utf8_lossy(&output);
    assert!(output.contains("--fix is currently only supported"));
}

#[test]
fn warns_and_fixes_engines_node() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("date.ts"), "Temporal.Now.instant();\n").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"fixture","engines":{"node":"^22.0.0"}}"#,
    )
    .unwrap();

    let warning = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let warning = String::from_utf8_lossy(&warning);
    let warning = visible_text(&warning);
    assert!(!warning.contains("Warnings"));
    assert!(warning.contains("⚠ Detected Node.js"));
    assert!(warning.contains("--fix"));

    command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .arg("--fix")
        .assert()
        .success();
    let package = fs::read_to_string(dir.path().join("package.json")).unwrap();
    assert!(package.contains(r#""node": ">=26.0.0""#));
}

#[test]
fn renders_stricter_engines_node_as_info() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("date.ts"), "fetch('/api');\n").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"fixture","engines":{"node":"^24.13.1"}}"#,
    )
    .unwrap();

    let output = command()
        .arg(dir.path())
        .arg("--runtime")
        .arg("node")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8_lossy(&output);
    let visible = visible_text(&output);
    assert!(!visible.contains("Warnings"));
    assert!(visible.contains("ⓘ Detected Node.js"));
    assert!(!visible.contains("⚠ Detected Node.js"));
}
