//! Command-line entry point for griz maintenance tasks.

fn main() -> anyhow::Result<()> {
    xtask::run(std::env::args().skip(1))
}
