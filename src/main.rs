use clap::Parser;

fn main() -> anyhow::Result<()> {
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "-h" || arg == "--help")
    {
        runtime_checker::print_help();
        return Ok(());
    }

    let cli = runtime_checker::Cli::parse();
    runtime_checker::run(cli)
}
