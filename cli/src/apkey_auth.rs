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
        let client = reqwest::Client::new();
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

async fn render_and_save_api_key(api_key: &str, config: &mut AppConfig) {
    if api_key.trim().is_empty() || !api_key.trim().starts_with("stkpk_api") {
        eprintln!("\nInvalid API key format.");
        eprintln!("API key must start with 'stkpk_api' and cannot be empty.");
        std::process::exit(1);
    }

    config.api_key = Some(api_key.trim().to_string());

    if let Err(e) = config.save() {
        eprintln!("Failed to save config: {}", e);
    }

    success_message();
}

async fn start_callback_server() -> (u16, mpsc::Receiver<String>, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    // Try to bind to port 5269, fallback to other ports if needed
    let mut port = 5269;
    let mut listener = None;

    while port < 5270 {
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
        None => panic!("Failed to bind to any port between 5269-5270"),
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

    (port, rx, server_handle)
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
    let (port, mut api_key_receiver, server_handle) = start_callback_server().await;

    let redirect_uri = format!("http://localhost:{}", port);
    let base_url = format!(
        "{}?redirect=true&response_type=code&client_id={}&redirect_uri={}",
        GENERATE_API_KEY_URL, CLIENT_ID, redirect_uri
    );

    println!();

    // Centered header with colored border and text
    println!("\x1b[1;36m┌──────────────────────────────────────────────────────────────┐\x1b[0m");
    println!(
        "\x1b[1;36m│\x1b[0m \x1b[1;33m                  Stakpak API Key Required                  \x1b[0m \x1b[1;36m│\x1b[0m"
    );
    println!("\x1b[1;36m└──────────────────────────────────────────────────────────────┘\x1b[0m");
    println!();
    println!("\x1b[1;34mBrowser did not open automatically? Use this url below to sign in\x1b[0m");
    println!();
    println!("{}", base_url);
    println!();

    // Clean separator
    println!("─────────────────────────────────────────────────────────────────────────────");
    println!();

    // Try to open browser first
    let browser_opened = open_browser(&base_url);

    if browser_opened {
        println!();
        // Give user option to wait for callback or enter manually
        println!(
            "Waiting for authorization...
        "
        );

        // Start callback polling in background
        let url_clone = base_url.to_string();
        let callback_handle = tokio::spawn(async move { listen_for_callback(&url_clone).await });

        // Use tokio::select! to wait for either the channel or a timeout
        tokio::select! {
            // Wait for API key from channel
            api_key = api_key_receiver.recv() => {
                match api_key {
                    Some(key) => {
                        callback_handle.abort(); // Cancel polling
                        server_handle.abort(); // Stop the server
                        render_and_save_api_key(&key, config).await;
                        return;
                    }
                    None => {
                        println!("❌ Server channel closed unexpectedly");
                    }
                }
            }
            // Timeout after 2 minutes
            _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                println!();
                println!("⏰ Timedout waiting for API key from server");
                println!();
                server_handle.abort(); // Stop the server
            }
        }

        // If we didn't get an API key from the channel, check if user wants to enter manually
        println!("Press Enter to enter API key manually, or wait for server...");
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_ok() && input.trim().is_empty() {
            // User pressed Enter, show manual prompt
            println!("🔄 Stopping server and switching to manual input...");
            println!();
            callback_handle.abort();
            server_handle.abort(); // Stop the server
        } else {
            // User typed something, fall back to polling
            println!("Checking polling result...");
            match callback_handle.await {
                Ok(result) => match result.as_str() {
                    "TIMEOUT" => {
                        println!("Callback timed out, switching to manual input...");
                        server_handle.abort(); // Stop the server
                    }
                    "ERROR" => {
                        println!("Callback failed, switching to manual input...");
                        server_handle.abort(); // Stop the server
                    }
                    key => {
                        let api_key = key.to_string();
                        server_handle.abort(); // Stop the server
                        render_and_save_api_key(&api_key, config).await;
                        return;
                    }
                },
                Err(_) => {
                    println!("Callback was cancelled, switching to manual input...");
                    server_handle.abort(); // Stop the server
                }
            }
        }
    } else {
        println!("❌ Browser could not be opened automatically");
        println!("Stopping server and switching to manual input...");
        println!();
        server_handle.abort(); // Stop the server
    }

    // Manual input path
    println!("Copy your API key (starts with '\x1b[1;32mstkpk_api\x1b[0m')");
    println!();
    print!("\x1b[1;34mPaste\x1b[0m your key here: ");
    if let Err(e) = std::io::stdout().flush() {
        eprintln!("Failed to flush stdout: {}", e);
        std::process::exit(1);
    }
    println!();
    println!();

    let manual_key = match rpassword::read_password() {
        Ok(key) => key,
        Err(e) => {
            eprintln!("\nFailed to read API key: {}", e);
            std::process::exit(1);
        }
    };

    render_and_save_api_key(&manual_key, config).await;

    // Clean success message
}
