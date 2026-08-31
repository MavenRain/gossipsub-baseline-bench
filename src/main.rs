//! CLI entry point: parse flags, run the benchmark, persist and print
//! the report. Composed as a comp-cat-rs `Io` program, run once at the
//! boundary.

use comp_cat_rs::effect::io::Io;
use gossipsub_baseline_bench::{config::BenchConfig, error::Error, report, runner};

fn main() {
    let program: Io<Error, String> = Io::suspend(|| {
        let cfg = BenchConfig::parse(std::env::args().skip(1))?;
        let summary = runner::run(cfg)?;
        report::persist(&summary)?;
        Ok(report::render(&summary))
    });
    program.run().map_or_else(
        |e| {
            eprintln!("error: {e}");
            std::process::exit(1)
        },
        |text| println!("{text}"),
    )
}
