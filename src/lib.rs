mod analyzer;
mod cli;
mod data;
mod engines;
mod generated;
mod help;
mod report;
mod scanner;
mod version;

use std::{collections::HashMap, time::Instant};

use anyhow::{Context, Result};

#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub use cli::{Cli, RuntimeKind};
use data::runtime;
use engines::check_engines;
pub use help::print_help;
use report::{ParserMode, Reporter, RuntimeReport};
use scanner::{DetectedFeature, FffMultiRuntimeScanner, Scanner, SourceDiscovery, SourceScan};

pub fn run(cli: Cli) -> Result<()> {
    let started = Instant::now();
    let root = cli
        .dir
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", cli.dir.display()))?;
    if cli.fix && !matches!(cli.runtime, RuntimeKind::All | RuntimeKind::Node) {
        anyhow::bail!("--fix is currently only supported for --runtime node or --runtime all");
    }

    let parser = if cli.fast {
        ParserMode::Text
    } else {
        ParserMode::Oxc
    };
    let aggregate = cli.inspect.is_none();
    let targets = cli.runtime.targets();
    let first_runtime = runtime(targets[0])?;
    let sources = SourceDiscovery.scan(&root, first_runtime)?;
    let stats = sources.stats();

    let reports = if cli.fast {
        scan_fast(&root, &sources, targets, aggregate, cli.fix)?
    } else {
        scan_ast(&root, sources, targets, aggregate, cli.fix)?
    };

    Reporter::new(cli.summary, cli.inspect, parser).print(
        &root,
        &reports,
        started.elapsed(),
        stats,
    );
    Ok(())
}

fn scan_fast(
    root: &std::path::Path,
    sources: &SourceScan,
    targets: &[RuntimeKind],
    aggregate: bool,
    fix: bool,
) -> Result<Vec<RuntimeReport>> {
    let target_runtimes = targets
        .iter()
        .copied()
        .map(runtime)
        .collect::<Result<Vec<_>>>()?;
    let scan_plan = ScanPlan::new(targets, &target_runtimes)?;
    let detections_by_runtime =
        FffMultiRuntimeScanner.scan_files(&scan_plan.runtimes, sources.files())?;
    let hidden_node_api_detected = scan_plan
        .hidden_node_index
        .and_then(|index| detections_by_runtime.get(index))
        .is_some_and(|detections| has_node_api_detections(detections));
    let mut reports = Vec::with_capacity(target_runtimes.len());

    for (runtime, detections) in target_runtimes.into_iter().zip(detections_by_runtime) {
        let node_api_detected = hidden_node_api_detected || has_node_api_detections(&detections);
        reports.push(build_runtime_report(
            root,
            runtime.name(),
            detections,
            node_api_detected,
            aggregate,
            fix,
        )?);
    }

    Ok(reports)
}

fn scan_ast(
    root: &std::path::Path,
    sources: SourceScan,
    targets: &[RuntimeKind],
    aggregate: bool,
    fix: bool,
) -> Result<Vec<RuntimeReport>> {
    let target_runtimes = targets
        .iter()
        .copied()
        .map(runtime)
        .collect::<Result<Vec<_>>>()?;
    let scan_plan = ScanPlan::new(targets, &target_runtimes)?;
    let detections_by_runtime =
        analyzer::analyze_files_for_runtimes(root, sources.files(), &scan_plan.runtimes)?;
    let hidden_node_api_detected = scan_plan
        .hidden_node_index
        .and_then(|index| detections_by_runtime.get(index))
        .is_some_and(|detections| has_node_api_detections(detections));
    let mut reports = Vec::with_capacity(targets.len());

    for (runtime, detections) in target_runtimes.into_iter().zip(detections_by_runtime) {
        let node_api_detected = hidden_node_api_detected || has_node_api_detections(&detections);
        reports.push(build_runtime_report(
            root,
            runtime.name(),
            detections,
            node_api_detected,
            aggregate,
            fix,
        )?);
    }

    Ok(reports)
}

fn build_runtime_report(
    root: &std::path::Path,
    runtime_name: &str,
    mut detections: Vec<DetectedFeature>,
    has_node_api_detections: bool,
    aggregate: bool,
    fix: bool,
) -> Result<RuntimeReport> {
    let minimum = detections
        .iter()
        .map(|detection| detection.version)
        .max()
        .unwrap_or_default();

    if aggregate {
        collapse_prefix_detections(&mut detections);
        detections = aggregate_feature_detections(detections);
    }

    let engines = if runtime_name == "node" {
        check_engines(root, minimum, fix)?
    } else {
        None
    };

    Ok(RuntimeReport {
        runtime: runtime_name.to_owned(),
        detections,
        minimum,
        engines,
        has_node_api_detections,
    })
}

struct ScanPlan<'a> {
    runtimes: Vec<&'a data::RuntimeDb>,
    hidden_node_index: Option<usize>,
}

impl<'a> ScanPlan<'a> {
    fn new(targets: &[RuntimeKind], target_runtimes: &[&'a data::RuntimeDb]) -> Result<Self> {
        let needs_hidden_node = targets.iter().any(|target| is_browser_runtime(*target))
            && !targets.contains(&RuntimeKind::Node);
        let mut runtimes = target_runtimes.to_vec();
        let hidden_node_index = if needs_hidden_node {
            let index = runtimes.len();
            runtimes.push(runtime(RuntimeKind::Node)?);
            Some(index)
        } else {
            None
        };

        Ok(Self {
            runtimes,
            hidden_node_index,
        })
    }
}

fn is_browser_runtime(runtime: RuntimeKind) -> bool {
    matches!(
        runtime,
        RuntimeKind::Safari | RuntimeKind::Chrome | RuntimeKind::Firefox
    )
}

fn has_node_api_detections(detections: &[DetectedFeature]) -> bool {
    detections
        .iter()
        .any(|detection| report::is_node_api_feature(&detection.feature))
}

fn aggregate_feature_detections(detections: Vec<DetectedFeature>) -> Vec<DetectedFeature> {
    let mut aggregated: Vec<DetectedFeature> = Vec::new();
    let mut indexes: HashMap<(String, crate::version::RuntimeVersion), usize> = HashMap::new();

    for detection in detections {
        let key = (detection.feature.clone(), detection.version);
        if let Some(index) = indexes.get(&key).copied() {
            aggregated[index].count += detection.count;
        } else {
            indexes.insert(key, aggregated.len());
            aggregated.push(detection);
        }
    }

    aggregated
}

fn collapse_prefix_detections(detections: &mut Vec<DetectedFeature>) {
    let mut by_location: HashMap<_, Vec<usize>> = HashMap::new();
    for (index, detection) in detections.iter().enumerate() {
        by_location
            .entry((detection.path.as_path(), detection.line, detection.column))
            .or_default()
            .push(index);
    }

    let mut remove = vec![false; detections.len()];
    for indices in by_location.values() {
        for &left in indices {
            for &right in indices {
                if left == right {
                    continue;
                }
                let prefix = format!("{}.", detections[left].feature);
                if detections[right].feature.starts_with(&prefix) {
                    remove[left] = true;
                }
            }
        }
    }

    let mut index = 0;
    detections.retain(|_| {
        let keep = !remove[index];
        index += 1;
        keep
    });
}
