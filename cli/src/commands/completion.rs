//! Shell completion script generation
//!
//! Generates completion scripts for the shells supported by [`clap_complete`].
//! Source the printed script in your shell's startup file to enable tab-completion
//! for every `stakpak` subcommand, flag, and argument.
//!
//! # Supported shells
//!
//! `bash`, `elvish`, `fish`, `powershell`, `zsh`
//!
//! # Quick setup
//!
//! ```bash
//! # Bash (add to ~/.bashrc)
//! source <(stakpak completion bash)
//!
//! # Zsh (add to ~/.zshrc)
//! source <(stakpak completion zsh)
//!
//! # Fish (add to ~/.config/fish/completions/)
//! stakpak completion fish > ~/.config/fish/completions/stakpak.fish
//! ```

use std::io;

use clap_complete::{Shell, generate};

/// Generate the completion script for `shell` and write it to stdout.
///
/// `cmd` must be the root [`clap::Command`] of the `stakpak` binary
/// (i.e. the value returned by `Cli::command()`).
pub fn print_completions(shell: Shell, cmd: &mut clap::Command) {
    generate(shell, cmd, cmd.get_name().to_string(), &mut io::stdout());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that completion scripts can be generated for every supported shell
    /// and that each script is non-empty.
    #[test]
    fn completion_output_is_non_empty_for_all_shells() {
        // Build a minimal command that mirrors the real CLI name so the generated
        // scripts reference the correct binary name.
        let mut cmd = clap::Command::new("stakpak")
            .subcommand(clap::Command::new("version"))
            .subcommand(
                clap::Command::new("completion").arg(clap::Arg::new("shell").required(true)),
            );

        for shell in [
            Shell::Bash,
            Shell::Elvish,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Zsh,
        ] {
            let mut buf = Vec::new();
            generate(shell, &mut cmd, "stakpak", &mut buf);
            assert!(
                !buf.is_empty(),
                "completion script for {shell} must be non-empty"
            );
            // All scripts must reference the binary name.
            let script = String::from_utf8(buf).expect("valid UTF-8");
            assert!(
                script.contains("stakpak"),
                "completion script for {shell} must reference the binary name"
            );
        }
    }
}
