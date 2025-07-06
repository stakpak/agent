use crate::utils::local_context::detect_container_environment;
use rand::Rng;
use tokio::net::TcpListener;

/// Reserved port range for ephemeral/dynamic ports (IANA recommendation)
const MIN_EPHEMERAL_PORT: u16 = 49152;
const MAX_EPHEMERAL_PORT: u16 = 65535;
const MAX_ATTEMPTS: u32 = 100;

/// Finds an available port using random selection to minimize collisions.
/// Returns a bound TcpListener that the caller must use immediately to avoid races.
pub async fn find_available_port_with_listener(host: &str) -> Result<(TcpListener, u16), String> {
    let mut rng = rand::rng();

    for _attempt in 0..MAX_ATTEMPTS {
        // Use random port selection to minimize collision probability
        let port = rng.random_range(MIN_EPHEMERAL_PORT..=MAX_EPHEMERAL_PORT);
        let addr = format!("{}:{}", host, port);

        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                return Ok((listener, port));
            }
            Err(_) => {
                // Port is in use, try another random port
                continue;
            }
        }
    }

    Err(format!(
        "Failed to find available port after {} attempts in range {}-{}",
        MAX_ATTEMPTS, MIN_EPHEMERAL_PORT, MAX_EPHEMERAL_PORT
    ))
}

// pub async fn find_available_port_descending(host: &str) -> Result<u16, String> {
//     let mut rng = rand::rng();

//     for _attempt in 0..MAX_ATTEMPTS {
//         let port = rng.random_range(MIN_EPHEMERAL_PORT..=MAX_EPHEMERAL_PORT);
//         let addr = format!("{}:{}", host, port);

//         match TcpListener::bind(&addr).await {
//             Ok(listener) => {
//                 // Drop the listener immediately to free the port
//                 // WARNING: This creates a race condition!
//                 drop(listener);
//                 return Ok(port);
//             }
//             Err(_) => continue,
//         }
//     }

//     Err(format!(
//         "No available port found after {} attempts in range {}-{}",
//         MAX_ATTEMPTS, MIN_EPHEMERAL_PORT, MAX_EPHEMERAL_PORT
//     ))
// }

// pub async fn find_available_bind_address_descending() -> Result<String, String> {
//     let host = match detect_container_environment() {
//         true => "0.0.0.0",
//         false => "localhost",
//     };
//     let port = find_available_port_descending(host).await?;
//     Ok(format!("{}:{}", host, port))
// }

/// Returns a bind address string and a bound TcpListener to prevent race conditions.
/// The caller must use the listener immediately to start their server.
pub async fn find_available_bind_address_with_listener() -> Result<(String, TcpListener), String> {
    let host = match detect_container_environment() {
        true => "0.0.0.0",
        false => "localhost",
    };

    let (listener, port) = find_available_port_with_listener(host).await?;
    let bind_address = format!("{}:{}", host, port);

    Ok((bind_address, listener))
}

// pub async fn start_mcp_server_with_reserved_port<F, Fut>(
//     server_starter: F,
// ) -> Result<String, String>
// where
//     F: FnOnce(TcpListener) -> Fut + Send + 'static,
//     Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send,
// {
//     let (bind_address, listener) = find_available_bind_address_with_listener().await?;

//     // Start the server with the pre-bound listener
//     tokio::spawn(async move {
//         if let Err(e) = server_starter(listener).await {
//             eprintln!("MCP server error: {}", e);
//         }
//     });

//     Ok(bind_address)
// }
