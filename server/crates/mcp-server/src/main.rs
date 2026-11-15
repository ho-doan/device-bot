//! Process entrypoint. Parsing completes before commands can start services.
mod assets;
mod bootstrap;
mod cli;
mod commands;
mod companion;
mod config;
mod flavor;
mod launcher;
mod qc;
mod qc_connection;
mod update;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    commands::execute(cli::parse(std::env::args().skip(1))?).await
}
