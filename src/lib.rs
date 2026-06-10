mod analyzer;
mod cli;
mod data;
mod engines;
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
    let runtimes = targets
        .iter()
        .copied()
        .map(runtime)
        .collect::<Result<Vec<_>>>()?;
    let detections_by_runtime = FffMultiRuntimeScanner.scan_files(&runtimes, sources.files())?;
    let mut reports = Vec::with_capacity(runtimes.len());

    for (runtime, detections) in runtimes.into_iter().zip(detections_by_runtime) {
        reports.push(build_runtime_report(
            root,
            runtime.name(),
            detections,
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
    let runtimes = targets
        .iter()
        .copied()
        .map(runtime)
        .collect::<Result<Vec<_>>>()?;
    let detections_by_runtime =
        analyzer::analyze_files_for_runtimes(root, sources.files(), &runtimes)?;
    let mut reports = Vec::with_capacity(targets.len());

    for (runtime, detections) in runtimes.into_iter().zip(detections_by_runtime) {
        reports.push(build_runtime_report(
            root,
            runtime.name(),
            detections,
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
    })
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
