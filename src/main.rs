use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use whashreonator::{cli::Cli, run};

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if let Err(error) = run(cli) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}
