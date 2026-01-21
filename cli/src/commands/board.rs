use clap::Subcommand;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;

/// v0.1.7 CLI structure: verb-first (list, create, update, delete, get)
#[derive(Subcommand, PartialEq)]
pub enum BoardCommands {
    /// Show version information
    Version,
    /// Get any entity by ID (auto-detects type from prefix)
    Get {
        /// Entity ID (board_*, card_*, agent_*)
        id: String,
        /// Output format: table, json, simple, pretty
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// List entities (boards, cards, agents)
    #[command(subcommand)]
    List(ListSubcommands),
    /// Create entities (boards, cards, agents, checklists, comments)
    #[command(subcommand)]
    Create(CreateSubcommands),
    /// Update entities (cards, agents, checklist items)
    #[command(subcommand)]
    Update(UpdateSubcommands),
    /// Delete entities (boards, cards, agents)
    #[command(subcommand)]
    Delete(DeleteSubcommands),
    /// Show your assigned cards
    Mine {
        /// Filter by status: todo, in-progress, pending-review, done
        #[arg(short, long)]
        status: Option<String>,
        /// Output format: table, json, simple, pretty
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Show current agent identity
    Whoami,
}

#[derive(Subcommand, PartialEq)]
pub enum ListSubcommands {
    /// List all boards
    Boards {
        /// Include deleted boards
        #[arg(long)]
        include_deleted: bool,
        /// Output format: table, json, simple, pretty
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// List cards on a board
    Cards {
        /// Board ID
        board_id: String,
        /// Filter by status
        #[arg(short, long)]
        status: Option<String>,
        /// Filter by tag (can be repeated, AND logic)
        #[arg(short, long, action = clap::ArgAction::Append)]
        tag: Vec<String>,
        /// Include deleted cards
        #[arg(long)]
        include_deleted: bool,
        /// Output format: table, json, simple, pretty
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// List all registered agents
    Agents {
        /// Output format: table, json, simple
        #[arg(short, long, default_value = "table")]
        format: String,
    },
}

#[derive(Subcommand, PartialEq)]
pub enum CreateSubcommands {
    /// Create a new board
    Board {
        /// Board name
        name: String,
        /// Board description
        #[arg(short, long)]
        description: Option<String>,
        /// Output format: table, json, simple
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Create a new card on a board
    Card {
        /// Board ID
        board_id: String,
        /// Card name
        name: String,
        /// Card description
        #[arg(short, long)]
        description: Option<String>,
        /// Initial status: todo, in-progress, pending-review, done
        #[arg(short, long, default_value = "todo")]
        status: String,
        /// Output format: table, json, simple
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Register a new agent identity
    Agent {
        /// Agent name (auto-generated if not provided)
        name: Option<String>,
        /// Command to invoke this agent (e.g., stakpak, claude)
        #[arg(long, default_value = "stakpak")]
        command: String,
        /// Agent description
        #[arg(long)]
        description: Option<String>,
        /// Output format: table, json, simple
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Add a checklist to a card
    Checklist {
        /// Card ID
        card_id: String,
        /// Checklist name
        #[arg(short, long, default_value = "Tasks")]
        name: String,
        /// Checklist items (can be repeated)
        #[arg(short, long, action = clap::ArgAction::Append)]
        item: Vec<String>,
        /// Output format: table, json, simple
        #[arg(short, long, default_value = "table")]
        format: String,
    },
    /// Add a comment to a card
    Comment {
        /// Card ID
        card_id: String,
        /// Comment text
        text: String,
        /// Output format: table, json, simple
        #[arg(short, long, default_value = "table")]
        format: String,
    },
}

#[derive(Subcommand, PartialEq)]
pub enum UpdateSubcommands {
    /// Update card fields
    Card {
        /// Card ID
        card_id: String,
        /// Update card name
        #[arg(long)]
        name: Option<String>,
        /// Update description
        #[arg(long)]
        description: Option<String>,
        /// Update status
        #[arg(short, long)]
        status: Option<String>,
        /// Assign to current agent
        #[arg(long)]
        assign_to_me: bool,
        /// Assign to specific agent
        #[arg(long)]
        assign: Option<String>,
        /// Add tag
        #[arg(long, action = clap::ArgAction::Append)]
        add_tag: Vec<String>,
        /// Remove tag
        #[arg(long, action = clap::ArgAction::Append)]
        remove_tag: Vec<String>,
    },
    /// Update agent details
    Agent {
        /// Agent ID
        agent_id: String,
        /// Set working directory
        #[arg(long)]
        workdir: Option<String>,
    },
    /// Check or uncheck a checklist item
    ChecklistItem {
        /// Item ID
        item_id: String,
        /// Check the item (mark as complete)
        #[arg(long)]
        check: bool,
        /// Uncheck the item (mark as incomplete)
        #[arg(long)]
        uncheck: bool,
    },
}

#[derive(Subcommand, PartialEq)]
pub enum DeleteSubcommands {
    /// Delete a board (soft delete)
    Board {
        /// Board ID
        board_id: String,
    },
    /// Delete a card (soft delete)
    Card {
        /// Card ID
        card_id: String,
    },
    /// Unregister an agent (soft delete)
    Agent {
        /// Agent ID
        agent_id: String,
    },
}

impl BoardCommands {
    pub async fn run(self) -> Result<(), String> {
        let board_path = get_board_plugin_path().await;
        let mut cmd = Command::new(board_path);

        match self {
            BoardCommands::Version => {
                cmd.arg("version");
            }
            BoardCommands::Get { id, format } => {
                cmd.arg("get").arg(&id);
                cmd.args(["--format", &format]);
            }
            BoardCommands::List(sub) => match sub {
                ListSubcommands::Boards {
                    include_deleted,
                    format,
                } => {
                    cmd.args(["list", "boards"]);
                    if include_deleted {
                        cmd.arg("--include-deleted");
                    }
                    cmd.args(["--format", &format]);
                }
                ListSubcommands::Cards {
                    board_id,
                    status,
                    tag,
                    include_deleted,
                    format,
                } => {
                    cmd.args(["list", "cards", &board_id]);
                    if let Some(s) = status {
                        cmd.args(["--status", &s]);
                    }
                    for t in tag {
                        cmd.args(["--tag", &t]);
                    }
                    if include_deleted {
                        cmd.arg("--include-deleted");
                    }
                    cmd.args(["--format", &format]);
                }
                ListSubcommands::Agents { format } => {
                    cmd.args(["list", "agents"]);
                    cmd.args(["--format", &format]);
                }
            },
            BoardCommands::Create(sub) => match sub {
                CreateSubcommands::Board {
                    name,
                    description,
                    format,
                } => {
                    cmd.args(["create", "board", &name]);
                    if let Some(desc) = description {
                        cmd.args(["--description", &desc]);
                    }
                    cmd.args(["--format", &format]);
                }
                CreateSubcommands::Card {
                    board_id,
                    name,
                    description,
                    status,
                    format,
                } => {
                    cmd.args(["create", "card", &board_id, &name]);
                    if let Some(desc) = description {
                        cmd.args(["--description", &desc]);
                    }
                    cmd.args(["--status", &status]);
                    cmd.args(["--format", &format]);
                }
                CreateSubcommands::Agent {
                    name,
                    command,
                    description,
                    format,
                } => {
                    cmd.args(["create", "agent"]);
                    if let Some(n) = name {
                        cmd.arg(&n);
                    }
                    cmd.args(["--command", &command]);
                    if let Some(d) = description {
                        cmd.args(["--description", &d]);
                    }
                    cmd.args(["--format", &format]);
                }
                CreateSubcommands::Checklist {
                    card_id,
                    name,
                    item,
                    format,
                } => {
                    cmd.args(["create", "checklist", &card_id]);
                    cmd.args(["--name", &name]);
                    for i in item {
                        cmd.args(["--item", &i]);
                    }
                    cmd.args(["--format", &format]);
                }
                CreateSubcommands::Comment {
                    card_id,
                    text,
                    format,
                } => {
                    cmd.args(["create", "comment", &card_id, &text]);
                    cmd.args(["--format", &format]);
                }
            },
            BoardCommands::Update(sub) => match sub {
                UpdateSubcommands::Card {
                    card_id,
                    name,
                    description,
                    status,
                    assign_to_me,
                    assign,
                    add_tag,
                    remove_tag,
                } => {
                    cmd.args(["update", "card", &card_id]);
                    if let Some(n) = name {
                        cmd.args(["--name", &n]);
                    }
                    if let Some(d) = description {
                        cmd.args(["--description", &d]);
                    }
                    if let Some(s) = status {
                        cmd.args(["--status", &s]);
                    }
                    if assign_to_me {
                        cmd.arg("--assign-to-me");
                    }
                    if let Some(a) = assign {
                        cmd.args(["--assign", &a]);
                    }
                    for t in add_tag {
                        cmd.args(["--add-tag", &t]);
                    }
                    for t in remove_tag {
                        cmd.args(["--remove-tag", &t]);
                    }
                }
                UpdateSubcommands::Agent { agent_id, workdir } => {
                    cmd.args(["update", "agent", &agent_id]);
                    if let Some(w) = workdir {
                        cmd.args(["--workdir", &w]);
                    }
                }
                UpdateSubcommands::ChecklistItem {
                    item_id,
                    check,
                    uncheck,
                } => {
                    cmd.args(["update", "checklist-item", &item_id]);
                    if check {
                        cmd.arg("--check");
                    }
                    if uncheck {
                        cmd.arg("--uncheck");
                    }
                }
            },
            BoardCommands::Delete(sub) => match sub {
                DeleteSubcommands::Board { board_id } => {
                    cmd.args(["delete", "board", &board_id]);
                }
                DeleteSubcommands::Card { card_id } => {
                    cmd.args(["delete", "card", &card_id]);
                }
                DeleteSubcommands::Agent { agent_id } => {
                    cmd.args(["delete", "agent", &agent_id]);
                }
            },
            BoardCommands::Mine { status, format } => {
                cmd.arg("mine");
                if let Some(s) = status {
                    cmd.args(["--status", &s]);
                }
                cmd.args(["--format", &format]);
            }
            BoardCommands::Whoami => {
                cmd.arg("whoami");
            }
        }

        execute_board_command(cmd)
    }
}

async fn get_board_plugin_path() -> String {
    // Check if we have an existing installation first
    let existing = get_existing_board_path().ok();
    let current_version = existing
        .as_ref()
        .and_then(|path| get_board_version(path).ok());

    // If we have an existing installation, check if update needed
    if let Some(ref path) = existing {
        // Try to get latest version from GitHub API
        match get_latest_github_release_version().await {
            Ok(target_version) => {
                if let Some(ref current) = current_version {
                    if is_version_match(current, &target_version) {
                        // Already up to date, use existing
                        return path.clone();
                    }
                    println!(
                        "agent-board {} is outdated (target: {}), updating...",
                        current, target_version
                    );
                }
                // Need to update - download new version
                match download_board_plugin().await {
                    Ok(new_path) => {
                        println!("Successfully installed agent-board {} -> {}", target_version, new_path);
                        return new_path;
                    }
                    Err(e) => {
                        eprintln!("Failed to update agent-board: {}", e);
                        eprintln!("Using existing version");
                        return path.clone();
                    }
                }
            }
            Err(_) => {
                // Can't check version, use existing installation
                return path.clone();
            }
        }
    }

    // No existing installation - must download
    match get_latest_github_release_version().await {
        Ok(target_version) => {
            match download_board_plugin().await {
                Ok(path) => {
                    println!("Successfully installed agent-board {} -> {}", target_version, path);
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-board: {}", e);
                    "agent-board".to_string()
                }
            }
        }
        Err(e) => {
            // Try download anyway (uses /latest/ URL)
            eprintln!("Warning: Failed to check version: {}", e);
            match download_board_plugin().await {
                Ok(path) => {
                    println!("Successfully installed agent-board -> {}", path);
                    path
                }
                Err(e) => {
                    eprintln!("Failed to download agent-board: {}", e);
                    "agent-board".to_string()
                }
            }
        }
    }
}

async fn get_latest_github_release_version() -> Result<String, String> {
    use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};

    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get("https://api.github.com/repos/stakpak/agent-board/releases/latest")
        .header("User-Agent", "stakpak-cli")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch latest release: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("GitHub API returned: {}", response.status()));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse GitHub response: {}", e))?;

    json["tag_name"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No tag_name in release".to_string())
}

fn get_existing_board_path() -> Result<String, String> {
    let home_dir = std::env::var("HOME")
        .map_err(|_| "HOME environment variable not set".to_string())?;

    let plugin_path = std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins")
        .join("agent-board");

    if plugin_path.exists() {
        Ok(plugin_path.to_string_lossy().to_string())
    } else {
        Err("agent-board not found in plugins directory".to_string())
    }
}

fn get_board_version(path: &str) -> Result<String, String> {
    let output = std::process::Command::new(path)
        .arg("version")
        .output()
        .map_err(|e| format!("Failed to run agent-board version: {}", e))?;

    if !output.status.success() {
        return Err("agent-board version command failed".to_string());
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    // Parse version from output like "agent-board v0.1.6" or just "v0.1.6"
    let trimmed = version_output.trim();
    if let Some(v) = trimmed.split_whitespace().find(|s| s.starts_with('v') || s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)) {
        Ok(v.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn is_version_match(current: &str, target: &str) -> bool {
    let current_clean = current.strip_prefix('v').unwrap_or(current);
    let target_clean = target.strip_prefix('v').unwrap_or(target);
    current_clean == target_clean
}

async fn download_board_plugin() -> Result<String, String> {
    use flate2::read::GzDecoder;
    use stakpak_shared::tls_client::{TlsClientConfig, create_tls_client};
    use std::io::Cursor;
    use tar::Archive;

    let home_dir = std::env::var("HOME")
        .map_err(|_| "HOME environment variable not set".to_string())?;

    let plugins_dir = std::path::PathBuf::from(&home_dir)
        .join(".stakpak")
        .join("plugins");

    std::fs::create_dir_all(&plugins_dir)
        .map_err(|e| format!("Failed to create plugins directory: {}", e))?;

    // Determine platform
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let target = match (os, arch) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        _ => return Err(format!("Unsupported platform: {} {}", os, arch)),
    };

    let download_url = format!(
        "https://github.com/stakpak/agent-board/releases/latest/download/agent-board-{}.tar.gz",
        target
    );

    println!("Downloading agent-board plugin...");

    let client = create_tls_client(TlsClientConfig::default())?;
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download agent-board: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let archive_bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;

    // Extract tar.gz
    let cursor = Cursor::new(archive_bytes.as_ref());
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    archive
        .unpack(&plugins_dir)
        .map_err(|e| format!("Failed to extract archive: {}", e))?;

    let plugin_path = plugins_dir.join("agent-board");

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&plugin_path)
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&plugin_path, permissions)
            .map_err(|e| format!("Failed to set executable permissions: {}", e))?;
    }

    Ok(plugin_path.to_string_lossy().to_string())
}

fn execute_board_command(mut cmd: Command) -> Result<(), String> {
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn agent-board process: {}", e))?;

    let stdout_handle = if let Some(stdout) = child.stdout.take() {
        let stdout_reader = BufReader::new(stdout);
        Some(thread::spawn(move || {
            for line in stdout_reader.lines() {
                match line {
                    Ok(line) => println!("{}", line),
                    Err(_) => break,
                }
            }
        }))
    } else {
        None
    };

    let stderr_handle = if let Some(stderr) = child.stderr.take() {
        let stderr_reader = BufReader::new(stderr);
        Some(thread::spawn(move || {
            for line in stderr_reader.lines() {
                match line {
                    Ok(line) => eprintln!("{}", line),
                    Err(_) => break,
                }
            }
        }))
    } else {
        None
    };

    let status = child
        .wait()
        .map_err(|e| format!("Failed to wait for agent-board process: {}", e))?;

    if let Some(handle) = stdout_handle {
        let _ = handle.join();
    }
    if let Some(handle) = stderr_handle {
        let _ = handle.join();
    }

    if !status.success() {
        return Err(format!(
            "agent-board command failed with exit code: {:?}",
            status.code()
        ));
    }

    Ok(())
}
