#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match vekai_core::cli::parse_cli_command(std::env::args()) {
        Ok(vekai_core::cli::CliCommand::Gui) => vekai_core::run(),
        Ok(vekai_core::cli::CliCommand::Serve) => {
            if let Err(error) = run_service_foreground() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        Ok(vekai_core::cli::CliCommand::InstallService) => {
            if let Err(error) = vekai_core::install_user_service() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        Ok(vekai_core::cli::CliCommand::UninstallService) => {
            if let Err(error) = vekai_core::uninstall_user_service() {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        Ok(vekai_core::cli::CliCommand::Help) => {
            vekai_core::print_cli_help();
        }
        Err(error) => {
            eprintln!("{error}");
            vekai_core::print_cli_help();
            std::process::exit(2);
        }
    }
}

fn run_service_foreground() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(vekai_core::run_service_foreground())
}
