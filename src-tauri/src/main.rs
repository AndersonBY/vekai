#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match local_rerank_server::cli::parse_cli_command(std::env::args()) {
        Ok(local_rerank_server::cli::CliCommand::Gui) => local_rerank_server::run(),
        Ok(local_rerank_server::cli::CliCommand::Serve) => {
            if let Err(error) = run_service_foreground() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        Ok(local_rerank_server::cli::CliCommand::InstallService) => {
            if let Err(error) = local_rerank_server::install_user_service() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        Ok(local_rerank_server::cli::CliCommand::UninstallService) => {
            if let Err(error) = local_rerank_server::uninstall_user_service() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        Ok(local_rerank_server::cli::CliCommand::Help) => {
            local_rerank_server::print_cli_help();
        }
        Err(error) => {
            eprintln!("{error}");
            local_rerank_server::print_cli_help();
            std::process::exit(2);
        }
    }
}

fn run_service_foreground() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(local_rerank_server::run_service_foreground())
}
