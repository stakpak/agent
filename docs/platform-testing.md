# Platform testing and Issue #243

This document collects cross-distribution testing notes, checklist and known workarounds related to https://github.com/stakpak/agent/issues/243.

## Testing checklist

- [ ] Test binary installation on Ubuntu 20.04
- [ ] Test binary installation on Ubuntu 22.04
- [ ] Test binary installation on Ubuntu 24.04
- [ ] Test binary installation on Debian 11
- [ ] Test binary installation on Debian 12
- [ ] Test Homebrew installation (if applicable)
- [ ] Verify TUI mode works correctly
- [ ] Verify MCP server mode works correctly
- [ ] Verify ACP mode works correctly
- [ ] Test auto-update functionality
- [ ] Validate Docker integration (docker run commands)
- [ ] Document any issues with dependencies or permissions

## Observed notes (from initial contributors)

- Debian/Ubuntu installations sometimes freeze while starting up after adding the API key. Removing `~/.stakpak` and reopening the app is a reported workaround.
- There is an `install.sh` script in the repo, but no documented uninstallation method. This repository now adds an uninstall section to `GETTING-STARTED.md`.
- Homebrew installation is provided in `GETTING-STARTED.md` but the website docs may not mention Homebrew explicitly — consider adding that to the docs site.
- Auto-update functionality isn't clearly visible in config or CLI help. The CLI exposes `stakpak update`. If you cannot find auto-update config, file an issue and attach `stakpak --version` and the output of `stakpak update`.
- ACP has moved to A2A; see the comments in issue #243 for migration notes.

## How to contribute test results

1. Run the checklist items locally on the target distro/version.
2. For each item, note the exact distro, kernel, and environment (WSL/container/VM/bare metal).
3. Attach logs, screenshots, and commands used.
4. Submit a PR or add a comment to issue #243 with your results.

Thanks for helping test Stakpak across platforms!
