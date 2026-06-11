use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.iter().any(|arg| arg == "-h" || arg == "--help") {
        runtime_checker::print_help();
        return Ok(());
    }

    let cli = runtime_checker::Cli::parse();
    runtime_checker::run(cli)
}
