use std::path::PathBuf;

use clap::Parser;
use paraflate_harness::{run_harness, HarnessConfig};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "test_file")]
    output: PathBuf,
    #[arg(long, default_value_t = 6)]
    level: u32,
    #[arg(long, default_value_t = 4)]
    threads: usize,
    #[arg(long, default_value_t = false)]
    skip_large: bool,
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();
    match run_harness(HarnessConfig {
        root: args.output,
        level: args.level,
        threads: args.threads,
        skip_large: args.skip_large,
    }) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e);
            std::process::ExitCode::from(1)
        }
    }
}
