use std::{collections::BTreeMap, path::Path, time::Duration};

use anstyle::{AnsiColor, Style};
use terminal_size::{Width, terminal_size};

use crate::{
    engines::{EnginesReport, EnginesSeverity},
    scanner::{DetectedFeature, ScanStats},
    version::RuntimeVersion,
};

pub struct RuntimeReport {
    pub runtime: String,
    pub detections: Vec<DetectedFeature>,
    pub minimum: RuntimeVersion,
    pub engines: Option<EnginesReport>,
    pub has_node_api_detections: bool,
}

pub struct Reporter {
    summary: bool,
    inspect: Option<String>,
    parser: ParserMode,
}

impl Reporter {
    pub fn new(summary: bool, inspect: Option<String>, parser: ParserMode) -> Self {
        Self {
            summary,
            inspect,
            parser,
        }
    }

    pub fn print(
        &self,
        root: &Path,
        reports: &[RuntimeReport],
        elapsed: Duration,
        stats: ScanStats,
    ) {
        if !self.summary {
            self.print_groups(root, reports);
        }

        print_result_panel(root, self.parser, reports, elapsed, stats);
    }

    fn print_groups(&self, root: &Path, reports: &[RuntimeReport]) {
        let mut printed_any = false;
        for report in reports {
            let printed = if let Some(feature) = &self.inspect {
                self.print_inspected_groups(root, report, feature, printed_any)
            } else {
                self.print_grouped_runtime(root, report, printed_any)
            };

            printed_any |= printed;
        }

        if !printed_any && let Some(feature) = &self.inspect {
            println!(
                "{}No detections found for {}{}{}",
                light_gray(),
                yellow(),
                feature,
                reset()
            );
        }

        if printed_any || self.inspect.is_some() {
            println!();
        }
    }

    fn print_grouped_runtime(
        &self,
        root: &Path,
        report: &RuntimeReport,
        needs_leading_blank: bool,
    ) -> bool {
        let mut groups: BTreeMap<u64, Vec<&DetectedFeature>> = BTreeMap::new();
        for detection in &report.detections {
            groups
                .entry(detection.version.major)
                .or_default()
                .push(detection);
        }

        let mut printed = false;
        for (_major, entries) in groups.iter_mut() {
            if needs_leading_blank || printed {
                println!();
            }

            println!(
                "{}",
                bold_fg_rgb(
                    format!(
                        "{} {}",
                        runtime_label(&report.runtime),
                        lowest_group_version(entries)
                    ),
                    WHITE
                )
            );

            let mut api_entries = Vec::new();
            let mut syntax_entries = Vec::new();
            let mut module_entries = Vec::new();
            let mut typescript_entries = Vec::new();
            for entry in entries.iter().copied() {
                match feature_category(&entry.feature) {
                    FeatureCategory::Api => api_entries.push(entry),
                    FeatureCategory::Syntax => syntax_entries.push(entry),
                    FeatureCategory::ModuleFormat => module_entries.push(entry),
                    FeatureCategory::NativeTypeScript => typescript_entries.push(entry),
                }
            }

            for entry in aggregate_entries(&api_entries) {
                print_aggregate(root, &entry);
            }

            if !syntax_entries.is_empty() {
                if !api_entries.is_empty() {
                    println!();
                }
                for entry in aggregate_entries(&syntax_entries) {
                    print_aggregate(root, &entry);
                }
            }

            print_spaced_entries(root, &module_entries);
            print_spaced_entries(root, &typescript_entries);

            printed = true;
        }

        printed
    }

    fn print_inspected_groups(
        &self,
        root: &Path,
        report: &RuntimeReport,
        feature: &str,
        needs_leading_blank: bool,
    ) -> bool {
        let mut groups: BTreeMap<u64, Vec<&DetectedFeature>> = BTreeMap::new();
        for detection in report
            .detections
            .iter()
            .filter(|detection| detection.feature == feature)
        {
            groups
                .entry(detection.version.major)
                .or_default()
                .push(detection);
        }

        if groups.is_empty() {
            return false;
        }

        let mut printed = false;
        for (_major, entries) in groups.iter_mut() {
            if needs_leading_blank || printed {
                println!();
            }
            entries.sort_by(|left, right| {
                left.version
                    .cmp(&right.version)
                    .then_with(|| left.path.cmp(&right.path))
                    .then_with(|| left.line.cmp(&right.line))
                    .then_with(|| left.column.cmp(&right.column))
            });

            println!(
                "{}",
                bold_fg_rgb(
                    format!(
                        "{} {}",
                        runtime_label(&report.runtime),
                        lowest_group_version(entries)
                    ),
                    WHITE
                )
            );
            for entry in entries {
                print_detection(root, entry);
            }
            printed = true;
        }

        printed
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ParserMode {
    Oxc,
    Text,
}

impl ParserMode {
    fn label(self) -> &'static str {
        match self {
            Self::Oxc => "oxc (ast parsing)",
            Self::Text => "fff (text scan)",
        }
    }
}

fn lowest_group_version(entries: &[&DetectedFeature]) -> RuntimeVersion {
    entries
        .iter()
        .map(|entry| entry.version)
        .min()
        .unwrap_or_default()
}

#[derive(Debug)]
struct AggregatedEntry<'a> {
    first: &'a DetectedFeature,
    count: usize,
}

fn aggregate_entries<'a>(entries: &[&'a DetectedFeature]) -> Vec<AggregatedEntry<'a>> {
    let mut by_feature: BTreeMap<(String, RuntimeVersion), AggregatedEntry<'a>> = BTreeMap::new();
    for entry in entries {
        let key = (entry.feature.clone(), entry.version);
        by_feature
            .entry(key)
            .and_modify(|aggregate| aggregate.count += entry.count)
            .or_insert(AggregatedEntry {
                first: entry,
                count: entry.count,
            });
    }

    let mut entries: Vec<_> = by_feature.into_values().collect();
    entries.sort_by(|left, right| {
        left.first
            .version
            .cmp(&right.first.version)
            .then_with(|| left.first.feature.cmp(&right.first.feature))
            .then_with(|| left.first.path.cmp(&right.first.path))
    });
    entries
}

fn print_aggregate(root: &Path, entry: &AggregatedEntry<'_>) {
    let path = entry
        .first
        .path
        .strip_prefix(root)
        .unwrap_or(&entry.first.path);
    let feature = feature_label(&entry.first.feature, entry.count);
    let location = format!(
        "({}@{}:{})",
        path.display(),
        entry.first.line,
        entry.first.column
    );

    println!(
        "{} {} {} {}{}",
        gradient(&feature, EMERALD_500, SKY_500),
        fg_rgb("•", NEUTRAL_700),
        fg_rgb(format!("v{}", entry.first.version), NEUTRAL_400),
        fg_rgb(location, NEUTRAL_500),
        reset()
    );
}

fn print_detection(root: &Path, entry: &DetectedFeature) {
    let path = entry.path.strip_prefix(root).unwrap_or(&entry.path);
    let feature = feature_label(&entry.feature, entry.count);
    let location = format!("({}@{}:{})", path.display(), entry.line, entry.column);

    println!(
        "{} {} {} {}{}",
        gradient(&feature, EMERALD_500, SKY_500),
        fg_rgb("•", NEUTRAL_700),
        fg_rgb(format!("v{}", entry.version), NEUTRAL_400),
        fg_rgb(location, NEUTRAL_500),
        reset()
    );
}

fn print_spaced_entries(root: &Path, entries: &[&DetectedFeature]) {
    if entries.is_empty() {
        return;
    }

    println!();
    for entry in aggregate_entries(entries) {
        print_aggregate(root, &entry);
    }
}

fn feature_label(feature: &str, count: usize) -> String {
    let feature = display_feature_label(feature).unwrap_or(feature);
    if count > 1 {
        format!("{feature} (x{count})")
    } else {
        feature.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureCategory {
    Api,
    Syntax,
    ModuleFormat,
    NativeTypeScript,
}

fn feature_category(feature: &str) -> FeatureCategory {
    if feature.starts_with("module.") {
        FeatureCategory::ModuleFormat
    } else if feature.starts_with("typescript.") {
        FeatureCategory::NativeTypeScript
    } else if feature.starts_with("syntax.") {
        FeatureCategory::Syntax
    } else {
        FeatureCategory::Api
    }
}

fn display_feature_label(feature: &str) -> Option<&'static str> {
    support_feature_label(feature).or_else(|| syntax_feature_label(feature))
}

fn support_feature_label(feature: &str) -> Option<&'static str> {
    match feature {
        "module.commonjs" => Some("CommonJS (CJS)"),
        "module.esm" => Some("ECMAScript modules (ESM)"),
        "module.iife" => Some("immediately invoked function expression (IIFE)"),
        "module.umd" => Some("universal module definition (UMD)"),
        "typescript.native" => Some("native TypeScript type stripping"),
        _ => None,
    }
}

fn syntax_feature_label(feature: &str) -> Option<&'static str> {
    match feature {
        "syntax.functions.arrow_functions" => Some("anonymous function (() => {})"),
        "syntax.functions.default_parameters" => Some("default parameters"),
        "syntax.functions.rest_parameters" => Some("rest parameters (...args)"),
        "syntax.grammar.array_literals" => Some("array literal ([...])"),
        "syntax.grammar.boolean_literals" => Some("boolean literal (true)"),
        "syntax.grammar.decimal_numeric_literals" => Some("number literal (1)"),
        "syntax.grammar.null_literal" => Some("null literal (null)"),
        "syntax.grammar.regular_expression_literals" => Some("regex literal (/.../)"),
        "syntax.grammar.string_literals" => Some("string literal (\"...\")"),
        "syntax.grammar.template_literals" => Some("template literal (`...`)"),
        "syntax.operators.addition" => Some("addition (+)"),
        "syntax.operators.addition_assignment" => Some("addition assignment (+=)"),
        "syntax.operators.assignment" => Some("assignment (=)"),
        "syntax.operators.async_function" => Some("async function (async function)"),
        "syntax.operators.async_generator_function" => {
            Some("async generator function (async function*)")
        }
        "syntax.operators.await" => Some("await expression (await)"),
        "syntax.operators.bitwise_and" => Some("bitwise and (&)"),
        "syntax.operators.bitwise_and_assignment" => Some("bitwise and assignment (&=)"),
        "syntax.operators.bitwise_not" => Some("bitwise not (~)"),
        "syntax.operators.bitwise_or" => Some("bitwise or (|)"),
        "syntax.operators.bitwise_or_assignment" => Some("bitwise or assignment (|=)"),
        "syntax.operators.bitwise_xor" => Some("bitwise xor (^)"),
        "syntax.operators.bitwise_xor_assignment" => Some("bitwise xor assignment (^=)"),
        "syntax.operators.class" => Some("class expression (class {})"),
        "syntax.operators.comma" => Some("sequence expression (,)"),
        "syntax.operators.conditional" => Some("conditional expression (a ? b : c)"),
        "syntax.operators.decrement" => Some("decrement (--)"),
        "syntax.operators.delete" => Some("delete operator (delete)"),
        "syntax.operators.division" => Some("division (/)"),
        "syntax.operators.division_assignment" => Some("division assignment (/=)"),
        "syntax.operators.equality" => Some("loose equality (==)"),
        "syntax.operators.exponentiation" => Some("exponentiation (**)"),
        "syntax.operators.exponentiation_assignment" => Some("exponentiation assignment (**=)"),
        "syntax.operators.function" => Some("function expression (function)"),
        "syntax.operators.generator_function" => Some("generator function (function*)"),
        "syntax.operators.greater_than" => Some("greater than (>)"),
        "syntax.operators.greater_than_or_equal" => Some("greater than or equal (>=)"),
        "syntax.operators.import" => Some("dynamic import (import())"),
        "syntax.operators.in" => Some("in operator (in)"),
        "syntax.operators.increment" => Some("increment (++)"),
        "syntax.operators.inequality" => Some("loose inequality (!=)"),
        "syntax.operators.instanceof" => Some("instanceof operator (instanceof)"),
        "syntax.operators.left_shift" => Some("left shift (<<)"),
        "syntax.operators.left_shift_assignment" => Some("left shift assignment (<<=)"),
        "syntax.operators.less_than" => Some("less than (<)"),
        "syntax.operators.less_than_or_equal" => Some("less than or equal (<=)"),
        "syntax.operators.logical_and" => Some("logical and (&&)"),
        "syntax.operators.logical_and_assignment" => Some("logical and assignment (&&=)"),
        "syntax.operators.logical_not" => Some("logical not (!)"),
        "syntax.operators.logical_or" => Some("logical or (||)"),
        "syntax.operators.logical_or_assignment" => Some("logical or assignment (||=)"),
        "syntax.operators.multiplication" => Some("multiplication (*)"),
        "syntax.operators.multiplication_assignment" => Some("multiplication assignment (*=)"),
        "syntax.operators.new" => Some("new expression (new)"),
        "syntax.operators.nullish_coalescing" => Some("nullish coalescing (??)"),
        "syntax.operators.nullish_coalescing_assignment" => Some("nullish assignment (??=)"),
        "syntax.operators.object_initializer" => Some("object literal ({})"),
        "syntax.operators.optional_chaining" => Some("optional chaining (?.)"),
        "syntax.operators.remainder" => Some("remainder (%)"),
        "syntax.operators.remainder_assignment" => Some("remainder assignment (%=)"),
        "syntax.operators.right_shift" => Some("right shift (>>)"),
        "syntax.operators.right_shift_assignment" => Some("right shift assignment (>>=)"),
        "syntax.operators.spread" => Some("spread syntax (...)"),
        "syntax.operators.strict_equality" => Some("strict equality (===)"),
        "syntax.operators.strict_inequality" => Some("strict inequality (!==)"),
        "syntax.operators.subtraction" => Some("subtraction (-)"),
        "syntax.operators.subtraction_assignment" => Some("subtraction assignment (-=)"),
        "syntax.operators.super" => Some("super expression (super)"),
        "syntax.operators.this" => Some("this expression (this)"),
        "syntax.operators.typeof" => Some("typeof operator (typeof)"),
        "syntax.operators.unary_negation" => Some("unary negation (-x)"),
        "syntax.operators.unary_plus" => Some("unary plus (+x)"),
        "syntax.operators.unsigned_right_shift" => Some("unsigned right shift (>>>)"),
        "syntax.operators.unsigned_right_shift_assignment" => {
            Some("unsigned right shift assignment (>>>=)")
        }
        "syntax.operators.void" => Some("void operator (void)"),
        "syntax.operators.yield" => Some("yield expression (yield)"),
        "syntax.statements.await_using" => Some("await using declaration (await using)"),
        "syntax.statements.async_function" => Some("async function declaration (async function)"),
        "syntax.statements.async_generator_function" => {
            Some("async generator function declaration (async function*)")
        }
        "syntax.statements.block" => Some("block statement ({})"),
        "syntax.statements.break" => Some("break statement (break)"),
        "syntax.statements.class" => Some("class declaration (class)"),
        "syntax.statements.const" => Some("constant declaration (const)"),
        "syntax.statements.continue" => Some("continue statement (continue)"),
        "syntax.statements.debugger" => Some("debugger statement (debugger)"),
        "syntax.statements.do_while" => Some("do while loop (do...while)"),
        "syntax.statements.empty" => Some("empty statement (;)"),
        "syntax.statements.export" => Some("export declaration (export)"),
        "syntax.statements.export.default" => Some("default export (export default)"),
        "syntax.statements.export.namespace" => Some("namespace export (export *)"),
        "syntax.statements.for" => Some("for loop (for)"),
        "syntax.statements.for_in" => Some("for in loop (for...in)"),
        "syntax.statements.for_of" => Some("for of loop (for...of)"),
        "syntax.statements.function" => Some("function declaration (function)"),
        "syntax.statements.generator_function" => {
            Some("generator function declaration (function*)")
        }
        "syntax.statements.if_else" => Some("if statement (if)"),
        "syntax.statements.import" => Some("import declaration (import)"),
        "syntax.statements.label" => Some("label statement (label:)"),
        "syntax.statements.let" => Some("let declaration (let)"),
        "syntax.statements.return" => Some("return statement (return)"),
        "syntax.statements.switch" => Some("switch statement (switch)"),
        "syntax.statements.throw" => Some("throw statement (throw)"),
        "syntax.statements.try_catch" => Some("try catch statement (try/catch)"),
        "syntax.statements.using" => Some("using declaration (using)"),
        "syntax.statements.var" => Some("variable declaration (var)"),
        "syntax.statements.while" => Some("while loop (while)"),
        "syntax.statements.with" => Some("with statement (with)"),
        _ => None,
    }
}

fn engines_notice(root: &Path, engines: &EnginesReport) -> PanelNotice {
    let package = engines
        .package_json
        .strip_prefix(root)
        .unwrap_or(&engines.package_json);
    if engines.fixed {
        PanelNotice {
            kind: NoticeKind::Update,
            message: format!(
                "Updated {} engines.node to >={}.",
                package.display(),
                engines.required
            ),
        }
    } else if let Some(declared) = &engines.declared {
        let kind = match engines.severity {
            EnginesSeverity::Info => NoticeKind::Info,
            EnginesSeverity::Warning => NoticeKind::Warning,
        };
        PanelNotice {
            kind,
            message: format!(
                "Detected Node.js {} but {} declares engines.node {}. Apply a fix with --fix.",
                engines.required,
                package.display(),
                declared
            ),
        }
    } else {
        PanelNotice {
            kind: NoticeKind::Warning,
            message: format!(
                "Detected Node.js {} but {} has no engines.node. Apply a fix with --fix.",
                engines.required,
                package.display()
            ),
        }
    }
}

fn print_result_panel(
    root: &Path,
    parser: ParserMode,
    reports: &[RuntimeReport],
    elapsed: Duration,
    stats: ScanStats,
) {
    let header = format!(
        "{} {}",
        gradient("runtime-checker", EMERALD_500, SKY_500),
        badge(env!("CARGO_PKG_VERSION"))
    );

    println!("{}", create_streak(&header));
    println!();
    println!(
        "{}{}{}{}{}{}{}",
        fg_rgb("Finished in ", WHITE),
        gradient(&format_duration(elapsed), EMERALD_500, SKY_500),
        fg_rgb(" using ", WHITE),
        gradient(parser.label(), EMERALD_500, SKY_500),
        fg_rgb(" after scanning ", WHITE),
        gradient(&format_line_count(stats.line_count), EMERALD_500, SKY_500),
        fg_rgb(" lines of code.", WHITE)
    );
    println!();

    let footnotes = compatibility_footnotes(reports);
    let printed_runtimes = print_summary_group(
        "Runtimes",
        reports
            .iter()
            .filter(|report| runtime_group(&report.runtime) == RuntimeGroup::Runtime),
        &footnotes,
    );
    if printed_runtimes
        && reports
            .iter()
            .any(|report| runtime_group(&report.runtime) == RuntimeGroup::Browser)
    {
        println!();
    }
    let printed_browsers = print_summary_group(
        "Browsers",
        reports
            .iter()
            .filter(|report| runtime_group(&report.runtime) == RuntimeGroup::Browser),
        &footnotes,
    );

    let printed_summary = printed_runtimes || printed_browsers;

    if !footnotes.is_empty() {
        if printed_summary {
            println!();
        }
        for footnote in &footnotes {
            println!(
                "{}",
                fg_rgb(
                    format!("{} {}", footnote_marker(footnote.number), footnote.message),
                    WARNING_RED
                )
            );
        }
    }

    let notices = panel_notices(root, reports);
    if !notices.is_empty() {
        if printed_summary || !footnotes.is_empty() {
            println!();
        }
        print_notice_group(&notices);
    }

    println!("{}", create_rule());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    Info,
    Warning,
    Update,
}

#[derive(Debug)]
struct PanelNotice {
    kind: NoticeKind,
    message: String,
}

fn panel_notices(root: &Path, reports: &[RuntimeReport]) -> Vec<PanelNotice> {
    reports
        .iter()
        .filter_map(|report| report.engines.as_ref())
        .map(|engines| engines_notice(root, engines))
        .collect()
}

fn print_notice_group(notices: &[PanelNotice]) {
    for notice in notices {
        let color = match notice.kind {
            NoticeKind::Info => NEUTRAL_400,
            NoticeKind::Warning => WARNING_RED,
            NoticeKind::Update => NODE_GREEN,
        };
        let icon = match notice.kind {
            NoticeKind::Info => "ⓘ",
            NoticeKind::Warning => "▲",
            NoticeKind::Update => "✓",
        };
        println!(
            "{} {}{}",
            fg_rgb(icon, color),
            fg_rgb(&notice.message, color),
            reset()
        );
    }
}

fn print_summary_group<'a>(
    title: &str,
    reports: impl Iterator<Item = &'a RuntimeReport>,
    footnotes: &[CompatibilityFootnote],
) -> bool {
    let reports: Vec<_> = reports.collect();
    if reports.is_empty() {
        return false;
    }

    println!("{}", bold_fg_rgb(title, WHITE));
    for report in reports {
        let marker = footnotes
            .iter()
            .find(|footnote| footnote.runtime == report.runtime)
            .map(|footnote| fg_rgb(footnote_marker(footnote.number), WARNING_RED))
            .unwrap_or_default();
        println!(
            "{} {}{} {}{}",
            fg_rgb("-", NEUTRAL_700),
            fg_rgb(runtime_label(&report.runtime), NEUTRAL_400),
            marker,
            fg_rgb(
                report.minimum.to_string(),
                runtime_version_color(&report.runtime),
            ),
            reset()
        );
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeGroup {
    Runtime,
    Browser,
}

fn runtime_group(runtime: &str) -> RuntimeGroup {
    match runtime {
        "safari" | "chrome" | "firefox" => RuntimeGroup::Browser,
        _ => RuntimeGroup::Runtime,
    }
}

fn footnote_marker(number: usize) -> String {
    let mut marker = String::new();
    for ch in number.to_string().chars() {
        let superscript = match ch {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            _ => return format!("[{number}]"),
        };
        marker.push(superscript);
    }
    marker
}

struct CompatibilityFootnote {
    runtime: String,
    number: usize,
    message: String,
}

fn compatibility_footnotes(reports: &[RuntimeReport]) -> Vec<CompatibilityFootnote> {
    if !has_node_api_detections(reports) {
        return Vec::new();
    }

    reports
        .iter()
        .filter(|report| runtime_group(&report.runtime) == RuntimeGroup::Browser)
        .enumerate()
        .map(|(index, report)| CompatibilityFootnote {
            runtime: report.runtime.clone(),
            number: index + 1,
            message: format!(
                "{} does not support Node APIs.",
                runtime_label(&report.runtime)
            ),
        })
        .collect()
}

fn has_node_api_detections(reports: &[RuntimeReport]) -> bool {
    reports.iter().any(|report| report.has_node_api_detections)
}

pub(crate) fn is_node_api_feature(feature: &str) -> bool {
    let root = feature.split('.').next().unwrap_or(feature);
    matches!(
        root,
        "__dirname"
            | "__filename"
            | "assert"
            | "async_hooks"
            | "Buffer"
            | "buffer"
            | "child_process"
            | "cluster"
            | "diagnostics_channel"
            | "dns"
            | "domain"
            | "events"
            | "fs"
            | "fsPromises"
            | "http"
            | "http2"
            | "https"
            | "module"
            | "net"
            | "os"
            | "path"
            | "perf_hooks"
            | "process"
            | "punycode"
            | "querystring"
            | "readline"
            | "repl"
            | "require"
            | "stream"
            | "string_decoder"
            | "timers"
            | "timersPromises"
            | "tls"
            | "tty"
            | "url"
            | "util"
            | "v8"
            | "vm"
            | "wasi"
            | "worker_threads"
            | "zlib"
    )
}

fn terminal_width() -> usize {
    terminal_size()
        .map(|(Width(width), _)| width as usize)
        .unwrap_or(80)
        .max(40)
}

type Rgb = (u8, u8, u8);

const EMERALD_500: Rgb = (16, 185, 129);
const SKY_500: Rgb = (14, 165, 233);
const NEUTRAL_400: Rgb = (163, 163, 163);
const NEUTRAL_500: Rgb = (115, 115, 115);
const NEUTRAL_700: Rgb = (64, 64, 64);
const NEUTRAL_600: Rgb = (82, 82, 82);
const WARNING_RED: Rgb = (248, 113, 113);
const NODE_GREEN: Rgb = (104, 160, 99);
const WHITE: Rgb = (255, 255, 255);

fn runtime_label(runtime: &str) -> &'static str {
    match runtime {
        "node" => "Node.js",
        "deno" => "Deno",
        "bun" => "Bun",
        "safari" => "Safari",
        "chrome" => "Chromium",
        "firefox" => "Firefox",
        _ => "Runtime",
    }
}

fn runtime_version_color(_runtime: &str) -> Rgb {
    WHITE
}

fn create_streak(content: &str) -> String {
    let columns = terminal_width().saturating_sub(1).max(1);
    let content_width = visible_width(content);
    let right_width = columns.saturating_sub(content_width + 3).max(1);

    format!(
        "{} {} {}",
        fg_rgb("─", NEUTRAL_700),
        content,
        fg_rgb("─".repeat(right_width), NEUTRAL_700)
    )
}

fn create_rule() -> String {
    let columns = terminal_width().saturating_sub(1).max(1);
    fg_rgb("─".repeat(columns), NEUTRAL_700)
}

fn gradient(text: &str, from: Rgb, to: Rgb) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return String::new();
    }

    let denominator = chars.len().saturating_sub(1).max(1) as f32;
    chars
        .into_iter()
        .enumerate()
        .map(|(index, ch)| {
            let amount = index as f32 / denominator;
            let (red, green, blue) = interpolate_rgb(from, to, amount);
            format!("\x1b[38;2;{red};{green};{blue}m{ch}\x1b[0m")
        })
        .collect()
}

fn badge(label: &str) -> String {
    format!(
        "\x1b[48;2;{};{};{}m\x1b[38;2;{};{};{}m {} \x1b[0m",
        NEUTRAL_600.0, NEUTRAL_600.1, NEUTRAL_600.2, WHITE.0, WHITE.1, WHITE.2, label
    )
}

fn fg_rgb(text: impl AsRef<str>, color: Rgb) -> String {
    format!(
        "\x1b[38;2;{};{};{}m{}\x1b[0m",
        color.0,
        color.1,
        color.2,
        text.as_ref()
    )
}

fn bold_fg_rgb(text: impl AsRef<str>, color: Rgb) -> String {
    format!(
        "\x1b[1;38;2;{};{};{}m{}\x1b[0m",
        color.0,
        color.1,
        color.2,
        text.as_ref()
    )
}

fn interpolate_rgb(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let red = from.0 as f32 + (to.0 as f32 - from.0 as f32) * amount;
    let green = from.1 as f32 + (to.1 as f32 - from.1 as f32) * amount;
    let blue = from.2 as f32 + (to.2 as f32 - from.2 as f32) * amount;
    (red.round() as u8, green.round() as u8, blue.round() as u8)
}

fn visible_width(value: &str) -> usize {
    let mut width = 0;
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

        width += 1;
    }

    width
}

fn format_line_count(line_count: usize) -> String {
    if line_count < 1_000 {
        return line_count.to_string();
    }

    let units = [
        (1_000_000_000_000usize, "t"),
        (1_000_000_000usize, "b"),
        (1_000_000usize, "m"),
        (1_000usize, "k"),
    ];

    for (index, (scale, suffix)) in units.iter().enumerate() {
        if line_count < *scale {
            continue;
        }

        let value = format_count_with_unit(line_count, *scale, suffix);
        if value.starts_with("1000") && index > 0 {
            let (next_scale, next_suffix) = units[index - 1];
            return format_count_with_unit(line_count, next_scale, next_suffix);
        }
        return value;
    }

    line_count.to_string()
}

fn format_count_with_unit(line_count: usize, scale: usize, suffix: &str) -> String {
    let value = line_count as f64 / scale as f64;
    let rounded = if value < 100.0 {
        (value * 10.0).round() / 10.0
    } else {
        value.round()
    };

    let text = if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    };

    format!("{text}{suffix}")
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }

    let seconds = duration.as_secs_f64();
    if seconds < 60.0 {
        return format_duration_unit(seconds, "s");
    }

    let minutes = seconds / 60.0;
    if minutes < 60.0 {
        return format_duration_unit(minutes, "m");
    }

    format_duration_unit(minutes / 60.0, "h")
}

fn format_duration_unit(value: f64, suffix: &str) -> String {
    let rounded = if value < 10.0 {
        (value * 10.0).round() / 10.0
    } else {
        value.round()
    };

    let text = if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    };

    format!("{text}{suffix}")
}

fn yellow() -> Style {
    Style::new().fg_color(Some(AnsiColor::Yellow.into()))
}

fn light_gray() -> Style {
    Style::new().fg_color(Some(AnsiColor::White.into()))
}

fn reset() -> impl std::fmt::Display + Copy {
    Style::new().render_reset()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{format_duration, format_line_count};

    #[test]
    fn formats_line_counts_without_ceiling_buckets() {
        assert_eq!(format_line_count(0), "0");
        assert_eq!(format_line_count(2), "2");
        assert_eq!(format_line_count(999), "999");
        assert_eq!(format_line_count(1_000), "1k");
        assert_eq!(format_line_count(1_234), "1.2k");
        assert_eq!(format_line_count(12_345), "12.3k");
        assert_eq!(format_line_count(82_314), "82.3k");
        assert_eq!(format_line_count(100_500), "101k");
        assert_eq!(format_line_count(999_500), "1m");
        assert_eq!(format_line_count(1_250_000), "1.3m");
    }

    #[test]
    fn formats_elapsed_duration_compactly() {
        assert_eq!(format_duration(Duration::from_millis(39)), "39ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
        assert_eq!(format_duration(Duration::from_millis(2_323)), "2.3s");
        assert_eq!(format_duration(Duration::from_secs(12)), "12s");
        assert_eq!(format_duration(Duration::from_secs(125)), "2.1m");
    }
}
