use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = runtime_checker::Cli::parse();
    runtime_checker::run(cli)
}
