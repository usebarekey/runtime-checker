use std::{collections::BTreeMap, path::Path, time::Duration};

use anstyle::{AnsiColor, Style};
use terminal_size::{Width, terminal_size};

use crate::{
    engines::EnginesReport,
    scanner::{DetectedFeature, ScanStats},
    version::RuntimeVersion,
};

pub struct RuntimeReport {
    pub runtime: String,
    pub detections: Vec<DetectedFeature>,
    pub minimum: RuntimeVersion,
    pub engines: Option<EnginesReport>,
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

        print_result_panel(self.parser, reports, elapsed, stats);

        if !self.summary {
            for report in reports {
                if let Some(engines) = &report.engines {
                    print_engines_report(root, engines);
                }
            }
        }
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

            for entry in aggregate_entries(entries) {
                print_aggregate(root, &entry);
            }

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

fn feature_label(feature: &str, count: usize) -> String {
    if count > 1 {
        format!("{feature} (x{count})")
    } else {
        feature.to_string()
    }
}

fn print_engines_report(root: &Path, engines: &EnginesReport) {
    let package = engines
        .package_json
        .strip_prefix(root)
        .unwrap_or(&engines.package_json);
    if engines.fixed {
        println!(
            "{}Updated {} engines.node to {}>={}.{}",
            green(),
            package.display(),
            yellow(),
            engines.required,
            reset()
        );
    } else if let Some(declared) = &engines.declared {
        println!(
            "{}Warning: detected Node.js {}{}{} but {} declares engines.node {}{}{}. Apply a fix with --fix.{}",
            light_gray(),
            yellow(),
            engines.required,
            light_gray(),
            package.display(),
            yellow(),
            declared,
            light_gray(),
            reset()
        );
    } else {
        println!(
            "{}Warning: detected Node.js {}{}{} but {} has no engines.node. Apply a fix with --fix.{}",
            light_gray(),
            yellow(),
            engines.required,
            light_gray(),
            package.display(),
            reset()
        );
    }
}

fn print_result_panel(
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
    let printed_browsers = print_summary_group(
        "Browsers",
        reports
            .iter()
            .filter(|report| runtime_group(&report.runtime) == RuntimeGroup::Browser),
        &footnotes,
    );

    if printed_runtimes || printed_browsers {
        println!();
    }

    if !footnotes.is_empty() {
        for footnote in &footnotes {
            println!(
                "{}",
                fg_rgb(
                    format!("{} {}", footnote_marker(footnote.number), footnote.message),
                    WARNING_RED
                )
            );
        }
        println!();
    }

    println!("{}", create_rule());
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
            fg_rgb(
                runtime_label(&report.runtime),
                runtime_theme_color(&report.runtime)
            ),
            marker,
            fg_rgb(
                report.minimum.to_string(),
                runtime_version_color(&report.runtime),
            ),
            reset()
        );
    }
    println!();
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
    reports
        .iter()
        .find(|report| report.runtime == "node")
        .is_some_and(|report| {
            report
                .detections
                .iter()
                .any(|detection| is_node_api_feature(&detection.feature))
        })
}

fn is_node_api_feature(feature: &str) -> bool {
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
const DENO_CYAN: Rgb = (112, 255, 175);
const BUN_ORANGE: Rgb = (249, 160, 63);
const SAFARI_BLUE: Rgb = (10, 132, 255);
const CHROME_BLUE: Rgb = (66, 133, 244);
const FIREFOX_ORANGE: Rgb = (255, 113, 57);
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

fn runtime_theme_color(runtime: &str) -> Rgb {
    match runtime {
        "node" => NODE_GREEN,
        "deno" => DENO_CYAN,
        "bun" => BUN_ORANGE,
        "safari" => SAFARI_BLUE,
        "chrome" => CHROME_BLUE,
        "firefox" => FIREFOX_ORANGE,
        _ => SKY_500,
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

fn green() -> Style {
    Style::new().fg_color(Some(AnsiColor::Green.into()))
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
