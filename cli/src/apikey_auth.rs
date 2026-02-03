use crate::config::AppConfig;
use std::io::Write;
use tokio::sync::mpsc;

const CLIENT_ID: &str = "stakpak-cli";
const GENERATE_API_KEY_URL: &str = "https://stakpak.dev/generate-api-key";

fn open_browser(url: &str) -> bool {
    match open::that(url) {
        Ok(_) => {
            println!("🌐 Opening browser...");
            true
        }
        Err(_) => false,
    }
}

async fn listen_for_callback(url: &str) -> String {
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < std::time::Duration::from_secs(120) {
        let client = stakpak_shared::tls_client::create_tls_client(
            stakpak_shared::tls_client::TlsClientConfig::default(),
        )
        .unwrap_or_else(|_| reqwest::Client::new());
        let response = client.get(url).send().await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let response_text = match resp.text().await {
                    Ok(text) => text,
                    Err(_) => {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                };
                if response_text.contains("stkpk_api") && response_text.contains("success") {
                    let json: serde_json::Value = match serde_json::from_str(&response_text) {
                        Ok(json) => json,
                        Err(_) => {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                    };
                    return json["key"].to_string();
                } else if response_text.contains("ERROR") {
                    return "ERROR".to_string();
                } else {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
            _ => {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }

    // 2 minutes elapsed
    "TIMEOUT".to_string()
}

fn success_message() {
    println!();
    println!("\x1b[1;36m┌──────────────────────────────────────────────────────────────┐\x1b[0m");
    println!(
        "\x1b[1;36m│\x1b[0m \x1b[1;32m                 API Key Saved Successfully!                \x1b[0m \x1b[1;36m│\x1b[0m"
    );
    println!("\x1b[1;36m└──────────────────────────────────────────────────────────────┘\x1b[0m");
    println!();
    println!("You're all set! Opening Stakpak...");
    println!();
}

fn clear_terminal() {
    print!("\x1b[2J\x1b[H");
    if let Err(e) = std::io::stdout().flush() {
        eprintln!("Failed to clear terminal: {}", e);
    }
}

async fn render_and_save_api_key(api_key: &str, config: &mut AppConfig) {
    if api_key.trim().is_empty() || !api_key.trim().starts_with("stkpk_api") {
        eprintln!("\nInvalid API key format.");
        eprintln!("API key must start with 'stkpk_api' and cannot be empty.");
        std::process::exit(1);
    }

    config.api_key = Some(api_key.trim().to_string());

    if let Err(e) = config.save() {
        eprintln!("Failed to save config: {}", e);
        std::process::exit(1);
    }

    success_message();
    // add timeout for 2 seconds
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    clear_terminal();
}

async fn start_callback_server() -> (
    u16,
    mpsc::Receiver<String>,
    tokio::task::JoinHandle<()>,
    bool,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let mut port_error = false;
    // Simulate port binding failure for testing
    let mut port = 5269;
    let mut listener: Option<tokio::net::TcpListener> = None;

    while port < 5279 {
        match TcpListener::bind(format!("127.0.0.1:{}", port)).await {
            Ok(l) => {
                listener = Some(l);
                println!("Callback server listening on http://localhost:{}", port);
                break;
            }
            Err(_) => {
                port += 1;
            }
        }
    }

    let listener = match listener {
        Some(l) => l,
        None => {
            port_error = true;
            return (
                0,
                mpsc::channel::<String>(100).1,
                tokio::spawn(async {}),
                port_error,
            );
        }
    };

    // Create a channel for communication between server and terminal
    let (tx, rx) = mpsc::channel::<String>(100);

    // Spawn the server to run continuously
    let server_handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((mut socket, _addr)) => {
                    // println!("Received connection from: {}", addr);

                    // Read the HTTP request
                    let mut buffer = [0; 2048]; // Increased buffer for POST data
                    let n = match socket.read(&mut buffer).await {
                        Ok(n) => n,
                        Err(_) => continue,
                    };

                    let request = String::from_utf8_lossy(&buffer[..n]);
                    // let first_line = request.lines().next().unwrap_or("Unknown");
                    // println!("Received request: {}", first_line);

                    // Handle POST requests to root endpoint (any data format)
                    if request.contains("POST /") || request.contains("POST / HTTP") {
                        // Extract API key from POST body (any format)
                        if let Some(api_key) = extract_api_key_from_post_body(&request) {
                            // Send API key through channel to terminal
                            let _ = tx.send(api_key.clone()).await;

                            // Send success response with CORS headers
                            let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Type: text/plain\r\n\r\nOK";
                            socket.write_all(response.as_bytes()).await.ok();

                            // Server has done its job, break out of the loop
                            break;
                        } else {
                            // Send error response with CORS headers
                            let response = "HTTP/1.1 400 Bad Request\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Type: text/plain\r\n\r\nInvalid API key";
                            socket.write_all(response.as_bytes()).await.ok();
                            println!("❌ Invalid API key format in POST data");
                        }
                    } else if request.contains("OPTIONS") {
                        // Handle CORS preflight request
                        let response = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, GET, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\n\r\n";
                        socket.write_all(response.as_bytes()).await.ok();
                    } else {
                        // Unknown endpoint
                        let response = "HTTP/1.1 404 Not Found\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: text/plain\r\n\r\nNot Found";
                        socket.write_all(response.as_bytes()).await.ok();
                    }
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {}", e);
                    continue;
                }
            }
        }

        // Server is shutting down
        // println!("🔄 Callback server shutting down...");
    });

    (port, rx, server_handle, port_error)
}

fn extract_api_key_from_post_body(request: &str) -> Option<String> {
    // Look for API key in the POST body (any format)
    // Split request into headers and body
    if let Some(body_start) = request.find("\r\n\r\n") {
        let body = &request[body_start + 4..];

        // Look for any text that starts with "stkpk_api"
        for line in body.lines() {
            for word in line.split_whitespace() {
                if word.starts_with("stkpk_api") {
                    // Clean up the API key (remove trailing commas, etc.)
                    let clean_key =
                        word.trim_matches(|c| c == ',' || c == ' ' || c == '}' || c == '{');
                    return Some(clean_key.to_string());
                }
            }
        }

        // Also check for key=value patterns
        for line in body.lines() {
            if line.contains("key:") || line.contains("key=") {
                for part in line.split(&[':', '=', ' ', ',', '{', '}']) {
                    if part.trim().starts_with("stkpk_api") {
                        // Clean up the API key (remove trailing commas, etc.)
                        let clean_key = part
                            .trim()
                            .trim_matches(|c| c == ',' || c == ' ' || c == '}' || c == '{');
                        return Some(clean_key.to_string());
                    }
                }
            }
        }
    }
    None
}

pub async fn prompt_for_api_key(config: &mut AppConfig) {
    let (port, mut api_key_receiver, server_handle, port_error) = start_callback_server().await;

    let redirect_uri = format!("http://localhost:{}", port);
    let base_url = format!(
        "{}?redirect=true&response_type=code&client_id={}&redirect_uri={}",
        GENERATE_API_KEY_URL, CLIENT_ID, redirect_uri
    );

    println!();

    println!("\x1b[1;36m┌──────────────────────────────────────────────────────────────┐\x1b[0m");
    println!(
        "\x1b[1;36m│\x1b[0m \x1b[1;33m                  Stakpak API Key Required                  \x1b[0m \x1b[1;36m│\x1b[0m"
    );
    println!("\x1b[1;36m└──────────────────────────────────────────────────────────────┘\x1b[0m");
    println!();
    println!("\x1b[1;34mUse the link below to authorize or paste your key directly\x1b[0m");
    println!();
    println!("{}", base_url);
    println!();
    println!("─────────────────────────────────────────────────────────────────────────────");
    println!();

    let browser_opened = !port_error && open_browser(&base_url);

    if port_error {
        println!("⚠️ Could not start the local callback server. Paste the API key manually.");
    } else if browser_opened {
        println!("Follow the prompts in your browser or paste the key here at any time.");
    } else {
        println!(
            "❌ Browser could not be opened automatically. Use the link above or paste the key below."
        );
    }

    println!();
    println!("Copy your API key (starts with '\x1b[1;32mstkpk_api\x1b[0m')");
    println!();
    print!("\x1b[1;34mPaste\x1b[0m your key here: ");
    if let Err(e) = std::io::stdout().flush() {
        eprintln!("Failed to flush stdout: {}", e);
        std::process::exit(1);
    }
    println!();
    println!();

    let (manual_tx, mut manual_rx) = mpsc::channel::<Result<String, String>>(1);
    let manual_input_handle = tokio::spawn(async move {
        let read_res = tokio::task::spawn_blocking(rpassword::read_password).await;
        let _ = match read_res {
            Ok(Ok(key)) => manual_tx.send(Ok(key)).await,
            Ok(Err(e)) => manual_tx.send(Err(format!("{}", e))).await,
            Err(e) => manual_tx.send(Err(format!("{}", e))).await,
        };
    });

    let mut callback_rx_option = None;
    let callback_handle_option = if !port_error {
        let (callback_tx, callback_rx) = mpsc::channel::<String>(1);
        let url_clone = base_url.clone();
        let handle = tokio::spawn(async move {
            let result = listen_for_callback(&url_clone).await;
            let _ = callback_tx.send(result).await;
        });
        callback_rx_option = Some(callback_rx);
        Some(handle)
    } else {
        None
    };

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(120));
    tokio::pin!(timeout);
    let mut timeout_triggered = false;

    let selected_key: Option<String> = loop {
        tokio::select! {
            Some(key) = api_key_receiver.recv() => {
                println!("\n🔐 Received API key via local callback");
                break Some(key);
            }
            Some(manual_result) = manual_rx.recv() => {
                match manual_result {
                    Ok(key) => {
                        println!("\n🔑 Received API key from manual input");
                        break Some(key);
                    }
                    Err(err) => {
                        eprintln!("\nFailed to read API key: {}", err);
                        std::process::exit(1);
                    }
                }
            }
            callback_result = async {
                if let Some(rx) = callback_rx_option.as_mut() {
                    rx.recv().await
                } else {
                    std::future::pending::<Option<String>>().await
                }
            } => {
                match callback_result {
                    Some(result) => {
                        match result.as_str() {
                            "TIMEOUT" => {
                                println!("\n⏰ Waiting for authorization timed out. You can paste the key manually whenever you're ready.");
                            }
                            "ERROR" => {
                                println!("\n⚠️ Authorization attempt failed. Paste the key manually or try again in the browser.");
                            }
                            key => {
                                println!("\n🔐 Received API key from polling endpoint");
                                break Some(key.to_string());
                            }
                        }
                    }
                    None => {
                        callback_rx_option = None;
                    }
                }
            }
            _ = &mut timeout, if !timeout_triggered => {
                timeout_triggered = true;
                println!("\n⏳ Still waiting for browser authorization... paste the key here any time.");
            }
        }
    };

    if let Some(handle) = callback_handle_option {
        handle.abort();
    }
    manual_input_handle.abort();
    server_handle.abort();

    if let Some(key) = selected_key {
        render_and_save_api_key(&key, config).await;
    }
}

/// Perform API key authentication via browser redirect (for ACP Agent Auth).
///
/// This function is designed for programmatic use (no terminal interaction).
/// It starts a local callback server, opens the browser for authentication,
/// and waits for the API key to be returned via the callback.
///
/// Returns the API key on success, or an error message on failure.
pub async fn authenticate_with_browser_redirect() -> Result<String, String> {
    let (port, mut api_key_receiver, server_handle, port_error) = start_callback_server().await;

    if port_error {
        return Err("Could not start local callback server for authentication".to_string());
    }

    let redirect_uri = format!("http://localhost:{}", port);
    let auth_url = format!(
        "{}?redirect=true&response_type=code&client_id={}&redirect_uri={}",
        GENERATE_API_KEY_URL, CLIENT_ID, redirect_uri
    );

    // Try to open browser - log but don't fail if it doesn't work
    // (the auth URL will be available in logs for manual access)
    if open::that(&auth_url).is_err() {
        log::warn!(
            "Could not open browser automatically. Auth URL: {}",
            auth_url
        );
    } else {
        log::info!("Opened browser for authentication");
    }

    // Wait for callback with timeout (2 minutes)
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(120));
    tokio::pin!(timeout);

    let result = tokio::select! {
        Some(key) = api_key_receiver.recv() => {
            log::info!("Received API key via local callback");
            // Clean up the key (remove quotes if present from JSON parsing)
            let clean_key = key.trim().trim_matches('"').to_string();
            if clean_key.starts_with("stkpk_api") {
                Ok(clean_key)
            } else {
                Err(format!("Invalid API key format received: {}", clean_key))
            }
        }
        _ = &mut timeout => {
            Err("Authentication timed out after 2 minutes".to_string())
        }
    };

    // Clean up the server
    server_handle.abort();

    result
}
