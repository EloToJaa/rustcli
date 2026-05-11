mod cli;
mod config;
mod model;
mod output;
mod store;
mod vdir;

use anyhow::Result;

fn main() -> Result<()> {
    let cli = cli::Cli::parse_args();
    let config = config::Config::load(cli.config.as_deref())?;
    let mut app = store::AppStore::open(&config)?;
    cli::run(cli, &config, &mut app)
}
