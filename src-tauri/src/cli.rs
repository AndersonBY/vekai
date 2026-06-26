use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliCommand {
    Gui,
    Serve,
    InstallService,
    UninstallService,
    Help,
}

pub fn parse_cli_command<I, S>(args: I) -> Result<CliCommand, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    let _program = args.next();
    let Some(command) = args.next() else {
        return Ok(CliCommand::Gui);
    };

    match command.as_ref() {
        "serve" => Ok(CliCommand::Serve),
        "install-service" => Ok(CliCommand::InstallService),
        "uninstall-service" => Ok(CliCommand::UninstallService),
        "--help" | "-h" | "help" => Ok(CliCommand::Help),
        other => Err(format!("unknown command: {other}")),
    }
}

pub fn linux_systemd_unit(executable: &Path) -> String {
    let executable = systemd_exec_arg(executable);
    format!(
        r#"[Unit]
Description=Local AI Services embedding and rerank server

[Service]
Type=simple
ExecStart={} serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        executable
    )
}

fn systemd_exec_arg(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if !raw.contains(char::is_whitespace) && !raw.contains('"') && !raw.contains('\\') {
        return raw.into_owned();
    }

    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_cli_command_defaults_to_gui_without_subcommand() {
        assert_eq!(
            parse_cli_command(["local-rerank-server"]).expect("command should parse"),
            CliCommand::Gui
        );
    }

    #[test]
    fn parse_cli_command_recognizes_service_subcommands() {
        assert_eq!(
            parse_cli_command(["local-rerank-server", "serve"]).expect("command should parse"),
            CliCommand::Serve
        );
        assert_eq!(
            parse_cli_command(["local-rerank-server", "install-service"])
                .expect("command should parse"),
            CliCommand::InstallService
        );
        assert_eq!(
            parse_cli_command(["local-rerank-server", "uninstall-service"])
                .expect("command should parse"),
            CliCommand::UninstallService
        );
    }

    #[test]
    fn linux_systemd_unit_runs_same_binary_in_serve_mode() {
        let unit = linux_systemd_unit(Path::new("/usr/bin/local-rerank-server"));

        assert!(unit.contains("Description=Local AI Services embedding and rerank server"));
        assert!(!unit.contains("network-online.target"));
        assert!(unit.contains("ExecStart=/usr/bin/local-rerank-server serve"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
    }

    #[test]
    fn linux_systemd_unit_quotes_executable_paths_with_spaces() {
        let unit = linux_systemd_unit(Path::new("/opt/Local AI Services/local-rerank-server"));

        assert!(unit.contains("ExecStart=\"/opt/Local AI Services/local-rerank-server\" serve"));
    }
}
