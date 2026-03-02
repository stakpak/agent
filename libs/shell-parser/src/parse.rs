//! Tree-sitter based shell command parsing.
use tree_sitter::{Node, Parser};

/// Shell interpreters recognized for nested `-c` script detection.
const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh"];
const ENV_VALUED_ARGS: &[&str] = &[
    "-u",
    "--unset",
    "-S",
    "--split-string",
    "-C",
    "--chdir",
    "--block-signal",
    "--default-signal",
    "--ignore-signal",
];

const XARGS_VALUED_FLAGS: &[&str] = &[
    "-a",
    "--arg-file",
    "-d",
    "--delimiter",
    "-E",
    "--eof",
    "-I",
    "--replace",
    "-L",
    "--max-lines",
    "-l",
    "-n",
    "--max-args",
    "-P",
    "--max-procs",
    "--process-slot-var",
    "-s",
    "--max-chars",
];

// "-e",
// "-i",

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    /// None if the command is variable expansion like `$CMD`.
    pub name: Option<String>,
    pub args: Vec<String>,
    pub offset: usize,
}

pub fn parse(input: &str) -> Vec<ParsedCommand> {
    let mut all_commands = Vec::new();
    let mut scripts = vec![input.to_string()];

    while let Some(script) = scripts.pop() {
        let mut parser = Parser::new();

        if parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .is_err()
        {
            continue;
        }

        let Some(tree) = parser.parse(&script, None) else {
            continue;
        };

        let mut stack = vec![tree.root_node()];

        while let Some(node) = stack.pop() {
            if node.kind() == "command" {
                let cmd = extract_command_from_node(&script, &node);

                if let Some(inner) = extract_nested_script(&cmd) {
                    scripts.push(inner);
                }

                all_commands.push(cmd);
            }

            let count = node.child_count() as u32;
            for i in (0..count).rev() {
                if let Some(child) = node.child(i) {
                    stack.push(child);
                }
            }
        }
    }

    all_commands
}

fn extract_command_from_node(source: &str, node: &Node) -> ParsedCommand {
    let offset = node.start_byte();
    let mut name: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    let mut found_name = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "command_name" => {
                found_name = true;
                let name_child = child.child(0);
                match name_child {
                    Some(nc) if nc.kind() == "simple_expansion" || nc.kind() == "expansion" => {
                        name = None;
                    }
                    Some(nc) => {
                        name = Some(node_text(source, &nc).to_string());
                    }
                    None => {
                        name = Some(node_text(source, &child).to_string());
                    }
                }
            }
            "file_redirect" | "heredoc_redirect" | "herestring_redirect" | "comment" => {}
            _ => {
                if found_name {
                    args.push(extract_word_text(source, &child));
                }
            }
        }
    }

    ParsedCommand { name, args, offset }
}

fn extract_word_text(source: &str, node: &Node) -> String {
    match node.kind() {
        "string" => {
            let raw = node_text(source, node);
            strip_quotes(raw, '"')
        }
        "raw_string" => {
            // Single-quoted string ($'...'): strip surrounding quotes.
            let raw = node_text(source, node);
            strip_quotes(raw, '\'')
        }
        "concatenation" => {
            // Concatenation of multiple parts — join them.
            let mut result = String::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                result.push_str(&extract_word_text(source, &child));
            }
            result
        }
        "simple_expansion" | "expansion" => {
            // Variable expansion like $VAR or ${VAR} — preserve as-is.
            node_text(source, node).to_string()
        }
        _ => node_text(source, node).to_string(),
    }
}

fn strip_quotes(s: &str, quote: char) -> String {
    let s = s.trim();

    let content = if let Some(inner) = s.strip_prefix(quote).and_then(|s| s.strip_suffix(quote)) {
        inner
    } else if let Some(inner) = s.strip_prefix("$'").and_then(|s| s.strip_suffix('\'')) {
        inner
    } else {
        return s.to_string();
    };

    unescape(content, quote)
}

fn unescape(content: &str, quote: char) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && (next == quote || next == '\\')
        {
            result.push(chars.next().unwrap());
        } else {
            result.push(c);
        }
    }

    result
}

fn node_text<'a>(source: &'a str, node: &Node) -> &'a str {
    source.get(node.start_byte()..node.end_byte()).unwrap_or("")
}

fn extract_nested_script(command: &ParsedCommand) -> Option<String> {
    let name = command.name.as_deref()?;

    if SHELLS.contains(&name) {
        return extract_after_c_flag(&command.args);
    }

    if name == "env" {
        let mut iter = command.args.iter();
        while let Some(arg) = iter.next() {
            if arg.starts_with('-') {
                if ENV_VALUED_ARGS.contains(&arg.as_str()) {
                    iter.next(); // skip the flag's value
                }
                continue;
            }
            if arg.contains('=') {
                continue;
            }
            if SHELLS.contains(&arg.as_str()) {
                let remaining: Vec<String> = iter.cloned().collect();
                return extract_after_c_flag(&remaining);
            }
            break;
        }
    } else if name == "xargs" {
        let mut iter = command.args.iter();
        while let Some(arg) = iter.next() {
            if arg.starts_with('-') {
                if XARGS_VALUED_FLAGS.contains(&arg.as_str())
                    && let Some(next) = iter.clone().next()
                    && !next.starts_with('-')
                {
                    iter.next(); // consume optional value
                }
                continue;
            }

            if SHELLS.contains(&arg.as_str()) {
                let remaining: Vec<String> = iter.cloned().collect();
                return extract_after_c_flag(&remaining);
            }

            break;
        }
    }

    None
}

fn extract_after_c_flag(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-c" {
            return iter.next().cloned();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_command() {
        let commands = parse("ls -la");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("ls"));
        assert_eq!(commands[0].args, vec!["-la"]);
    }

    #[test]
    fn parse_command_with_multiple_args() {
        let commands = parse("rm -rf /tmp/foo");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("rm"));
        assert_eq!(commands[0].args, vec!["-rf", "/tmp/foo"]);
    }

    #[test]
    fn parse_pipeline() {
        let commands = parse("cat file.txt | grep pattern | wc -l");
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name.as_deref(), Some("cat"));
        assert_eq!(commands[1].name.as_deref(), Some("grep"));
        assert_eq!(commands[2].name.as_deref(), Some("wc"));
    }

    #[test]
    fn parse_and_chain() {
        let commands = parse("mkdir -p dir && cd dir && ls");
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name.as_deref(), Some("mkdir"));
        assert_eq!(commands[1].name.as_deref(), Some("cd"));
        assert_eq!(commands[2].name.as_deref(), Some("ls"));
    }

    #[test]
    fn parse_or_chain() {
        let commands = parse("test -f file || echo missing");
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name.as_deref(), Some("test"));
        assert_eq!(commands[1].name.as_deref(), Some("echo"));
    }

    #[test]
    fn parse_semicolon_list() {
        let commands = parse("echo a; echo b; echo c");
        assert_eq!(commands.len(), 3);
    }

    #[test]
    fn parse_command_substitution() {
        let commands = parse("echo $(whoami)");
        // Should find both echo and whoami
        assert!(commands.len() >= 2);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"whoami"));
    }

    #[test]
    fn parse_subshell() {
        let commands = parse("(cd /tmp && ls)");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"cd"));
        assert!(names.contains(&"ls"));
    }

    #[test]
    fn parse_nested_sh_c() {
        let commands = parse(r#"sh -c "rm -rf /tmp/foo""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"sh"));
        assert!(names.contains(&"rm"), "nested rm should be extracted");
    }

    #[test]
    fn parse_nested_bash_c() {
        let commands = parse(r#"bash -c "ls -la /home""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"ls"), "nested ls should be extracted");
    }

    #[test]
    fn parse_nested_env_bash_c() {
        let commands = parse(r#"env PATH=/usr/bin bash -c "echo hello""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"env"));
        assert!(names.contains(&"echo"), "nested echo should be extracted");
    }

    #[test]
    fn parse_nested_xargs_sh_c() {
        let commands = parse(r#"find . -name "*.log" | xargs sh -c "rm $1""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"find"));
        assert!(names.contains(&"xargs"));
        assert!(names.contains(&"rm"), "nested rm should be extracted");
    }

    #[test]
    fn parse_variable_expansion_command() {
        let commands = parse("$MY_CMD arg1 arg2");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, None);
    }

    #[test]
    fn parse_empty_string() {
        let commands = parse("");
        assert_eq!(commands.len(), 0);
    }

    #[test]
    fn parse_only_comment() {
        let commands = parse("# this is a comment");
        assert_eq!(commands.len(), 0);
    }

    #[test]
    fn parse_complex_pipeline_with_args() {
        let commands = parse("curl -X POST https://api.example.com | jq '.data' | head -n 5");
        assert_eq!(commands.len(), 3);
        assert_eq!(commands[0].name.as_deref(), Some("curl"));
        assert_eq!(commands[0].args[0], "-X");
        assert_eq!(commands[0].args[1], "POST");
    }

    #[test]
    fn extract_nested_script_sh() {
        let cmd = ParsedCommand {
            name: Some("sh".to_string()),
            args: vec!["-c".to_string(), "echo hello".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo hello".to_string()));
    }

    #[test]
    fn extract_nested_script_no_c_flag() {
        let cmd = ParsedCommand {
            name: Some("sh".to_string()),
            args: vec!["script.sh".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_env() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "VAR=val".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "ls".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("ls".to_string()));
    }

    #[test]
    fn extract_nested_script_not_shell() {
        let cmd = ParsedCommand {
            name: Some("ls".to_string()),
            args: vec!["-la".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn strip_quotes_double_quoted() {
        assert_eq!(strip_quotes(r#""hello world""#, '"'), "hello world");
    }

    #[test]
    fn strip_quotes_single_quoted() {
        assert_eq!(strip_quotes("'hello world'", '\''), "hello world");
    }

    #[test]
    fn strip_quotes_empty_double() {
        assert_eq!(strip_quotes(r#""""#, '"'), "");
    }

    #[test]
    fn strip_quotes_empty_single() {
        assert_eq!(strip_quotes("''", '\''), "");
    }

    #[test]
    fn strip_quotes_no_match_returns_as_is() {
        assert_eq!(strip_quotes("no quotes here", '"'), "no quotes here");
    }

    #[test]
    fn strip_quotes_mismatched_quotes() {
        // Starts with " but ends with ' — no stripping
        assert_eq!(strip_quotes("\"hello'", '"'), "\"hello'");
    }

    #[test]
    fn strip_quotes_escaped_inner_double() {
        assert_eq!(strip_quotes(r#""say \"hi\"""#, '"'), r#"say "hi""#);
    }

    #[test]
    fn strip_quotes_escaped_inner_single() {
        assert_eq!(strip_quotes(r"'it\'s'", '\''), "it's");
    }

    #[test]
    fn strip_quotes_escaped_backslash() {
        // \\  inside quotes should become single backslash
        assert_eq!(strip_quotes(r#""path\\to""#, '"'), r"path\to");
    }

    #[test]
    fn strip_quotes_dollar_single_quote_style() {
        assert_eq!(strip_quotes("$'hello'", '\''), "hello");
    }

    #[test]
    fn strip_quotes_dollar_single_quote_escaped() {
        assert_eq!(strip_quotes(r"$'it\'s'", '\''), "it's");
    }

    #[test]
    fn strip_quotes_dollar_single_quote_empty() {
        assert_eq!(strip_quotes("$''", '\''), "");
    }

    #[test]
    fn strip_quotes_whitespace_trimmed() {
        assert_eq!(strip_quotes("  \"hello\"  ", '"'), "hello");
    }

    #[test]
    fn strip_quotes_single_char_string() {
        assert_eq!(strip_quotes("\"", '"'), "\"");
    }

    #[test]
    fn strip_quotes_backslash_not_before_quote() {
        assert_eq!(strip_quotes(r#""hello\nworld""#, '"'), r"hello\nworld");
    }

    #[test]
    fn extract_after_c_flag_empty_args() {
        assert_eq!(extract_after_c_flag(&[]), None);
    }

    #[test]
    fn extract_after_c_flag_no_c_present() {
        let args = vec!["-x".to_string(), "foo".to_string()];
        assert_eq!(extract_after_c_flag(&args), None);
    }

    #[test]
    fn extract_after_c_flag_c_is_last_arg() {
        let args = vec!["-c".to_string()];
        assert_eq!(extract_after_c_flag(&args), None);
    }

    #[test]
    fn extract_after_c_flag_c_with_script() {
        let args = vec!["-c".to_string(), "echo hi".to_string()];
        assert_eq!(extract_after_c_flag(&args), Some("echo hi".to_string()));
    }

    #[test]
    fn extract_after_c_flag_c_with_extra_args() {
        // Only the first arg after -c is returned
        let args = vec!["-c".to_string(), "echo hi".to_string(), "arg0".to_string()];
        assert_eq!(extract_after_c_flag(&args), Some("echo hi".to_string()));
    }

    #[test]
    fn extract_after_c_flag_c_preceded_by_other_flags() {
        let args = vec!["-e".to_string(), "-c".to_string(), "ls".to_string()];
        assert_eq!(extract_after_c_flag(&args), Some("ls".to_string()));
    }

    #[test]
    fn extract_nested_script_none_name() {
        let cmd = ParsedCommand {
            name: None,
            args: vec!["-c".to_string(), "echo hi".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_all_shell_variants() {
        for shell in SHELLS {
            let cmd = ParsedCommand {
                name: Some(shell.to_string()),
                args: vec!["-c".to_string(), "echo test".to_string()],
                offset: 0,
            };
            assert_eq!(
                extract_nested_script(&cmd),
                Some("echo test".to_string()),
                "shell {shell} should support -c extraction"
            );
        }
    }

    #[test]
    fn extract_nested_script_xargs_with_shell() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec!["zsh".to_string(), "-c".to_string(), "echo ok".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo ok".to_string()));
    }

    #[test]
    fn extract_nested_script_env_no_shell_in_args() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "VAR=val".to_string(),
                "python".to_string(),
                "script.py".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_xargs_no_shell_in_args() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec!["rm".to_string(), "-f".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_shell_no_args() {
        let cmd = ParsedCommand {
            name: Some("bash".to_string()),
            args: vec![],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_env_shell_no_c_flag() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec!["bash".to_string(), "script.sh".to_string()],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_env_unset_flag_skipped() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "-u".to_string(),
                "FOO".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "echo hi".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo hi".to_string()));
    }

    #[test]
    fn extract_nested_script_env_chdir_flag_skipped() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "-C".to_string(),
                "/tmp".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "echo dir".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo dir".to_string()));
    }

    #[test]
    fn extract_nested_script_env_split_string_flag_skipped() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "-S".to_string(),
                "VAR=val cmd".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "echo split".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo split".to_string()));
    }

    #[test]
    fn extract_nested_script_env_key_val_assignment_skipped() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "VAR=val".to_string(),
                "OTHER=x".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "echo vars".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo vars".to_string()));
    }

    #[test]
    fn extract_nested_script_env_mixed_flags_and_assignments() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "-u".to_string(),
                "OLDVAR".to_string(),
                "NEW=1".to_string(),
                "-C".to_string(),
                "/home".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "echo mixed".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo mixed".to_string()));
    }

    #[test]
    fn extract_nested_script_env_non_shell_after_flags() {
        let cmd = ParsedCommand {
            name: Some("env".to_string()),
            args: vec![
                "-u".to_string(),
                "FOO".to_string(),
                "python".to_string(),
                "script.py".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn extract_nested_script_xargs_n_flag_skipped() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec![
                "-n".to_string(),
                "1".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "echo item".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo item".to_string()));
    }

    #[test]
    fn extract_nested_script_xargs_max_lines_flag_skipped() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec![
                "-L".to_string(),
                "1".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "rm {}".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("rm {}".to_string()));
    }

    #[test]
    fn extract_nested_script_xargs_multiple_flags_skipped() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec![
                "-P".to_string(),
                "4".to_string(),
                "-I".to_string(),
                "{}".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                "echo {}".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo {}".to_string()));
    }

    #[test]
    fn extract_nested_script_xargs_standalone_flags_skipped() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec![
                "-0".to_string(),
                "-t".to_string(),
                "bash".to_string(),
                "-c".to_string(),
                "echo null".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), Some("echo null".to_string()));
    }

    #[test]
    fn extract_nested_script_xargs_non_shell_after_flags() {
        let cmd = ParsedCommand {
            name: Some("xargs".to_string()),
            args: vec![
                "-n".to_string(),
                "1".to_string(),
                "rm".to_string(),
                "-f".to_string(),
            ],
            offset: 0,
        };
        assert_eq!(extract_nested_script(&cmd), None);
    }

    #[test]
    fn parse_whitespace_only() {
        let commands = parse("   \t\n  ");
        assert_eq!(commands.len(), 0);
    }

    #[test]
    fn parse_multiline_script() {
        let script = "echo first\necho second\necho third";
        let commands = parse(script);
        assert_eq!(commands.len(), 3);
        for cmd in &commands {
            assert_eq!(cmd.name.as_deref(), Some("echo"));
        }
    }

    #[test]
    fn parse_file_redirect_not_in_args() {
        let commands = parse("echo hello > output.txt");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("echo"));
        assert_eq!(commands[0].args, vec!["hello"]);
    }

    #[test]
    fn parse_input_redirect_not_in_args() {
        let commands = parse("sort < input.txt");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("sort"));
        // The redirect itself should not appear as an arg
        assert!(
            !commands[0]
                .args
                .iter()
                .any(|a| a.contains('<') || a == "input.txt"),
            "redirect targets should not be in args: {:?}",
            commands[0].args
        );
    }

    #[test]
    fn parse_append_redirect() {
        let commands = parse("echo line >> log.txt");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("echo"));
        assert_eq!(commands[0].args, vec!["line"]);
    }

    #[test]
    fn parse_background_command() {
        let commands = parse("sleep 10 &");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("sleep"));
        assert_eq!(commands[0].args, vec!["10"]);
    }

    #[test]
    fn parse_multiple_background_commands() {
        let commands = parse("cmd1 & cmd2 & cmd3");
        assert_eq!(commands.len(), 3);
    }

    #[test]
    fn parse_backtick_command_substitution() {
        let commands = parse("echo `whoami`");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"echo"));
        assert!(
            names.contains(&"whoami"),
            "backtick substitution should be extracted"
        );
    }

    #[test]
    fn parse_double_nested_command_substitution() {
        let commands = parse("echo $(cat $(find . -name foo))");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"cat"));
        assert!(names.contains(&"find"));
    }

    #[test]
    fn parse_curly_brace_expansion_command() {
        let commands = parse("${MY_CMD} arg1");
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0].name, None,
            "${{VAR}} command names should be None"
        );
    }

    #[test]
    fn parse_double_quoted_arg() {
        let commands = parse(r#"echo "hello world""#);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].args, vec!["hello world"]);
    }

    #[test]
    fn parse_single_quoted_arg() {
        let commands = parse("echo 'hello world'");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].args, vec!["hello world"]);
    }

    #[test]
    fn parse_mixed_operators() {
        let commands = parse("a && b || c; d | e");
        assert_eq!(commands.len(), 5);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert_eq!(names, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn parse_command_with_env_vars() {
        let commands = parse("FOO=bar BAZ=qux my_cmd arg1");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(
            names.contains(&"my_cmd"),
            "command with prefix env vars should be parsed"
        );
    }

    #[test]
    fn parse_nested_subshells() {
        let commands = parse("(echo a; (echo b; echo c))");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert_eq!(names.iter().filter(|&&n| n == "echo").count(), 3);
    }

    #[test]
    fn parse_heredoc() {
        let script = "cat <<EOF\nhello\nEOF";
        let commands = parse(script);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("cat"));
    }

    #[test]
    fn parse_no_args_command() {
        let commands = parse("pwd");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("pwd"));
        assert!(commands[0].args.is_empty());
    }

    #[test]
    fn parse_deeply_nested_sh_c() {
        let commands = parse(r#"sh -c "bash -c \"echo deep\"""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"sh"));
        assert!(
            names.contains(&"bash"),
            "first nesting level should be extracted"
        );
    }

    #[test]
    fn parse_variable_expansion_in_args() {
        let commands = parse("echo $HOME ${USER}");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("echo"));
        assert_eq!(commands[0].args.len(), 2);
        assert!(commands[0].args[0].contains("$HOME"));
        assert!(commands[0].args[1].contains("${USER}"));
    }

    #[test]
    fn parse_for_loop() {
        let commands = parse("for f in *.txt; do echo $f; done");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"echo"));
    }

    #[test]
    fn parse_while_loop() {
        let commands = parse("while true; do echo loop; sleep 1; done");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"sleep"));
    }

    #[test]
    fn parse_if_then_else() {
        let commands = parse("if test -f foo; then echo yes; else echo no; fi");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"test"));
        assert_eq!(names.iter().filter(|&&n| n == "echo").count(), 2);
    }

    #[test]
    fn parse_negated_command() {
        let commands = parse("! grep error log.txt");
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"grep"));
    }

    #[test]
    fn parse_command_with_inline_comment() {
        let commands = parse("echo hello # this is a comment");
        assert!(commands.len() >= 1);
        assert_eq!(commands[0].name.as_deref(), Some("echo"));
    }

    #[test]
    fn parse_offset_first_command() {
        let commands = parse("ls -la");
        assert_eq!(commands[0].offset, 0);
    }

    #[test]
    fn parse_offset_piped_commands() {
        let commands = parse("echo hi | grep hi");
        assert_eq!(commands[0].offset, 0);
        // "echo hi | grep hi"
        //            ^ offset 10
        assert!(commands[1].offset > 0);
    }

    #[test]
    fn parse_offset_semicolon_chain() {
        let commands = parse("a; bb; ccc");
        assert_eq!(commands[0].offset, 0);
        assert!(commands[1].offset > commands[0].offset);
        assert!(commands[2].offset > commands[1].offset);
    }

    #[test]
    fn parse_offset_preserves_leading_whitespace() {
        let commands = parse("   echo hi");
        // The command starts at byte 3 due to leading spaces
        assert_eq!(commands[0].offset, 3);
    }

    #[test]
    fn parse_concatenated_arg() {
        // e.g., echo foo"bar"baz should produce foobar joined
        let commands = parse(r#"echo foo"bar"baz"#);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].args.len(), 1);
        assert_eq!(commands[0].args[0], "foobarbaz");
    }

    #[test]
    fn parse_stderr_redirect_not_in_args() {
        let commands = parse("cmd arg1 2>/dev/null");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name.as_deref(), Some("cmd"));
        assert_eq!(commands[0].args, vec!["arg1"]);
    }

    #[test]
    fn parse_pipe_stderr() {
        let commands = parse("cmd1 2>&1 | cmd2");
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name.as_deref(), Some("cmd1"));
        assert_eq!(commands[1].name.as_deref(), Some("cmd2"));
    }

    #[test]
    fn parse_nested_zsh_c() {
        let commands = parse(r#"zsh -c "echo zsh""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"zsh"));
        assert!(names.contains(&"echo"));
    }

    #[test]
    fn parse_nested_dash_c() {
        let commands = parse(r#"dash -c "echo dash""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"dash"));
        assert!(names.contains(&"echo"));
    }

    #[test]
    fn parse_nested_fish_c() {
        let commands = parse(r#"fish -c "echo fish""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"fish"));
        assert!(names.contains(&"echo"));
    }

    #[test]
    fn parse_nested_ksh_c() {
        let commands = parse(r#"ksh -c "echo ksh""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"ksh"));
        assert!(names.contains(&"echo"));
    }

    // --- Integration tests for env/xargs flag-skipping through full parse() ---

    #[test]
    fn parse_env_unset_flag_then_shell() {
        let commands = parse(r#"env -u FOO bash -c "echo hi""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"env"));
        assert!(
            names.contains(&"echo"),
            "nested echo should be extracted after env -u FOO"
        );
    }

    #[test]
    fn parse_env_chdir_flag_then_shell() {
        let commands = parse(r#"env -C /tmp bash -c "echo dir""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"env"));
        assert!(
            names.contains(&"echo"),
            "nested echo should be extracted after env -C /tmp"
        );
    }

    #[test]
    fn parse_env_key_val_then_shell() {
        let commands = parse(r#"env VAR=val bash -c "echo $VAR""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"env"));
        assert!(
            names.contains(&"echo"),
            "nested echo should be extracted after env VAR=val"
        );
    }

    #[test]
    fn parse_xargs_n_flag_then_shell() {
        let commands = parse(r#"find . | xargs -n 1 bash -c "echo item""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"find"));
        assert!(names.contains(&"xargs"));
        assert!(
            names.contains(&"echo"),
            "nested echo should be extracted after xargs -n 1"
        );
    }

    #[test]
    fn parse_xargs_max_lines_flag_then_shell() {
        let commands = parse(r#"find . | xargs -L 1 bash -c "rm {}""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"find"));
        assert!(names.contains(&"xargs"));
        assert!(
            names.contains(&"rm"),
            "nested rm should be extracted after xargs -L 1"
        );
    }

    #[test]
    fn parse_xargs_multiple_flags_then_shell() {
        let commands = parse(r#"find . | xargs -P 4 -I {} sh -c "echo {}""#);
        let names: Vec<_> = commands.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"xargs"));
        assert!(
            names.contains(&"echo"),
            "nested echo should be extracted after xargs -P 4 -I {{}}"
        );
    }
}
