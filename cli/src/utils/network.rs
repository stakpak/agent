/// Finds an available port starting from 65535 and decrementing until an available port is found.
pub async fn find_available_port_descending() -> Result<u16, String> {
    for port in (1024..=65535).rev() {
        let addr = format!("0.0.0.0:{}", port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                // Drop the listener immediately, just checking availability
                drop(listener);
                return Ok(port);
            }
            Err(_) => continue, // Port is in use, try the next one
        }
    }
    Err("No available port found in range 1024-65535".to_string())
}

/// Returns a bind address string with the first available port starting from 65535 and decrementing.
pub async fn find_available_bind_address_descending() -> Result<String, String> {
    let port = find_available_port_descending().await?;
    Ok(format!("0.0.0.0:{}", port))
}
