use terminal_size::{Width, terminal_size};

const LABEL_COLUMN_WIDTH: usize = 28;

type Rgb = (u8, u8, u8);

const EMERALD_500: Rgb = (16, 185, 129);
const SKY_500: Rgb = (14, 165, 233);
const NEUTRAL_500: Rgb = (115, 115, 115);
const NEUTRAL_600: Rgb = (82, 82, 82);
const NEUTRAL_700: Rgb = (64, 64, 64);
const WHITE: Rgb = (255, 255, 255);

pub fn print_help() {
    print!("{}", create_help_body());
}

fn create_help_body() -> String {
    let body = [
        create_header(),
        create_section("Usage", &[usage_row()]),
        create_section("Arguments", &[argument_row()]),
        create_section(
            "Options",
            &[
                option_row(
                    &["--fast"],
                    "use FFF text scanning; faster but less precise",
                ),
                option_row(
                    &["--runtime <runtime>"],
                    "target all, node, deno, bun, safari, chrome, or firefox",
                ),
                option_row(&["--summary"], "print only the summary panel"),
                option_row(
                    &["--inspect <feature>"],
                    "print every detection for one feature",
                ),
                option_row(&["--fix"], "update package.json engines.node when useful"),
                option_row(&["-h", "--help"], "display help for command"),
            ],
        ),
    ]
    .into_iter()
    .filter(|section| !section.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n");

    format!("{body}\n")
}

fn create_header() -> String {
    let wordmark = gradient("runtime-checker", EMERALD_500, SKY_500);
    let version = badge(env!("CARGO_PKG_VERSION"));
    create_streak(&format!("{wordmark} {version}"))
}

fn usage_row() -> String {
    format!(
        "  {} {}",
        gradient("runtime-checker", EMERALD_500, SKY_500),
        fg_rgb("<dir> [options]", WHITE)
    )
}

fn argument_row() -> String {
    let label = gradient("<dir>", EMERALD_500, SKY_500);
    format!(
        "  {}{}",
        pad_ansi(&label, LABEL_COLUMN_WIDTH),
        fg_rgb("directory to scan", WHITE)
    )
}

fn option_row(flags: &[&str], description: &str) -> String {
    let flags = format_aliases(flags);
    format!(
        "  {}{}",
        pad_ansi(&flags, LABEL_COLUMN_WIDTH),
        fg_rgb(description, WHITE)
    )
}

fn create_section(title: &str, rows: &[String]) -> String {
    if rows.is_empty() {
        return String::new();
    }

    let heading = format!("{} {}", fg_rgb(title, WHITE), fg_rgb("»", NEUTRAL_700));
    let mut section = heading;
    for row in trim_empty_edges(rows) {
        section.push('\n');
        section.push_str(row);
    }
    section
}

fn format_aliases(aliases: &[&str]) -> String {
    let separator = fg_rgb(", ", NEUTRAL_500);
    aliases
        .iter()
        .map(|alias| gradient(alias, EMERALD_500, SKY_500))
        .collect::<Vec<_>>()
        .join(&separator)
}

fn trim_empty_edges(rows: &[String]) -> &[String] {
    let Some(start) = rows.iter().position(|row| !row.is_empty()) else {
        return &[];
    };
    let end = rows
        .iter()
        .rposition(|row| !row.is_empty())
        .unwrap_or(start);
    &rows[start..=end]
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

fn terminal_width() -> usize {
    terminal_size()
        .map(|(Width(width), _)| width as usize)
        .unwrap_or(80)
        .max(40)
}

fn badge(label: &str) -> String {
    format!(
        "\x1b[48;2;{};{};{}m\x1b[38;2;{};{};{}m {label} \x1b[0m",
        NEUTRAL_600.0, NEUTRAL_600.1, NEUTRAL_600.2, WHITE.0, WHITE.1, WHITE.2
    )
}

fn fg_rgb(text: impl AsRef<str>, color: Rgb) -> String {
    let text = text.as_ref();
    format!(
        "\x1b[38;2;{};{};{}m{text}\x1b[0m",
        color.0, color.1, color.2
    )
}

fn gradient(text: &str, from: Rgb, to: Rgb) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let denominator = chars.len().saturating_sub(1).max(1) as f32;
    chars
        .iter()
        .enumerate()
        .map(|(index, ch)| {
            let amount = index as f32 / denominator;
            let red = interpolate(from.0, to.0, amount);
            let green = interpolate(from.1, to.1, amount);
            let blue = interpolate(from.2, to.2, amount);
            format!("\x1b[38;2;{red};{green};{blue}m{ch}\x1b[0m")
        })
        .collect()
}

fn interpolate(from: u8, to: u8, amount: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * amount).round() as u8
}

fn pad_ansi(value: &str, width: usize) -> String {
    let padding = width.saturating_sub(visible_width(value));
    format!("{value}{}", " ".repeat(padding))
}

fn visible_width(value: &str) -> usize {
    let mut width = 0;
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            width += 1;
        }
    }
    width
}
