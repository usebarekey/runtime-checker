use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RuntimeKind {
    All,
    Node,
    Deno,
    Bun,
    Safari,
    Chrome,
    Firefox,
}

const ALL_RUNTIMES: &[RuntimeKind] = &[
    RuntimeKind::Node,
    RuntimeKind::Deno,
    RuntimeKind::Bun,
    RuntimeKind::Safari,
    RuntimeKind::Chrome,
    RuntimeKind::Firefox,
];
const NODE_RUNTIME: &[RuntimeKind] = &[RuntimeKind::Node];
const DENO_RUNTIME: &[RuntimeKind] = &[RuntimeKind::Deno];
const BUN_RUNTIME: &[RuntimeKind] = &[RuntimeKind::Bun];
const SAFARI_RUNTIME: &[RuntimeKind] = &[RuntimeKind::Safari];
const CHROME_RUNTIME: &[RuntimeKind] = &[RuntimeKind::Chrome];
const FIREFOX_RUNTIME: &[RuntimeKind] = &[RuntimeKind::Firefox];

impl RuntimeKind {
    pub fn targets(self) -> &'static [RuntimeKind] {
        match self {
            Self::All => ALL_RUNTIMES,
            Self::Node => NODE_RUNTIME,
            Self::Deno => DENO_RUNTIME,
            Self::Bun => BUN_RUNTIME,
            Self::Safari => SAFARI_RUNTIME,
            Self::Chrome => CHROME_RUNTIME,
            Self::Firefox => FIREFOX_RUNTIME,
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "runtime-checker")]
#[command(about = "Detect the minimum runtime version required by a codebase")]
pub struct Cli {
    /// Directory to scan.
    pub dir: PathBuf,

    /// Use text matching only. This is intentionally less reliable.
    #[arg(long)]
    pub fast: bool,

    /// Runtime compatibility target.
    #[arg(long, value_enum, default_value_t = RuntimeKind::All)]
    pub runtime: RuntimeKind,

    /// Print only the summary panel.
    #[arg(long, alias = "oneline")]
    pub summary: bool,

    /// Print every detection for one feature instead of grouped summaries.
    #[arg(long, value_name = "FEATURE")]
    pub inspect: Option<String>,

    /// Update package.json engines.node when it is missing or too low.
    #[arg(long)]
    pub fix: bool,
}
