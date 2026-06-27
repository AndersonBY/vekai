# Linux Service Mode

The same Vekai binary can run as a GUI tray app or as a foreground service process.

```bash
vekai serve
vekai install-service
vekai uninstall-service
```

`install-service` installs a user-level systemd unit at:

```text
~/.config/systemd/user/vekai.service
```

It uses the same user config and data directories as the GUI app. To start the service before the user logs in, enable lingering for that user:

```bash
loginctl enable-linger "$USER"
```

The llama.cpp runtime archive and GGUF models are not embedded in the installer. Download the matching runtime archive from the GitHub Release assets or run:

```bash
pwsh ./scripts/download-assets.ps1 -RuntimePlatform ubuntu-arm64
```
