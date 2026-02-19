use std::fmt::Write;
use std::process::Command;

/// Discover listening TCP ports on the local machine.
/// Uses pure /proc parsing on Linux, lsof on macOS, netstat on Windows.
pub fn discover() -> String {
    let os = std::env::consts::OS;
    match os {
        "linux" => discover_linux(),
        "macos" => discover_macos(),
        "windows" => discover_windows(),
        _ => discover_fallback(),
    }
}

/// Linux: parse /proc/net/tcp and /proc/net/tcp6 directly — no external deps.
fn discover_linux() -> String {
    let mut ports = Vec::new();

    for proto_file in &["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(content) = std::fs::read_to_string(proto_file) {
            for line in content.lines().skip(1) {
                // Fields: sl local_address rem_address st ...
                let fields: Vec<&str> = line.split_whitespace().collect();
                if fields.len() < 4 {
                    continue;
                }
                // st == "0A" means LISTEN
                if fields[3] != "0A" {
                    continue;
                }
                // local_address is hex_ip:hex_port
                if let Some(port_hex) = fields[1].split(':').nth(1)
                    && let Ok(port) = u16::from_str_radix(port_hex, 16)
                {
                    // Parse the hex IP to determine bind address
                    let addr = parse_proc_addr(fields[1], proto_file.contains("tcp6"));
                    ports.push((port, addr));
                }
            }
        }
    }

    if ports.is_empty() {
        return "(no listening ports detected)\n".to_string();
    }

    ports.sort_by_key(|(port, _)| *port);
    ports.dedup();

    let mut out = String::with_capacity(ports.len() * 40);
    for (port, addr) in &ports {
        let _ = writeln!(out, "- {}:{}", addr, port);
    }
    out
}

/// Parse hex address from /proc/net/tcp format into human-readable form.
fn parse_proc_addr(hex_addr: &str, is_v6: bool) -> String {
    let parts: Vec<&str> = hex_addr.split(':').collect();
    if parts.is_empty() {
        return "?".to_string();
    }
    let ip_hex = parts[0];

    if is_v6 {
        if ip_hex == "00000000000000000000000000000000" {
            return "[::]".to_string();
        }
        if ip_hex == "00000000000000000000FFFF00000000" || ip_hex.ends_with("00000000") {
            return "0.0.0.0".to_string();
        }
        return "[::...]".to_string();
    }

    // IPv4: hex is in little-endian
    if ip_hex.len() == 8
        && let Ok(num) = u32::from_str_radix(ip_hex, 16)
    {
        let bytes = num.to_le_bytes();
        return format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]);
    }
    "?".to_string()
}

/// macOS: use lsof (always available).
fn discover_macos() -> String {
    let output = match Command::new("lsof")
        .args(["-iTCP", "-sTCP:LISTEN", "-P", "-n"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(_) | Err(_) => return "(failed to run lsof)\n".to_string(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<String> = Vec::new();

    for line in stdout.lines().skip(1) {
        // COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 9 {
            continue;
        }
        let command = fields[0];
        let pid = fields[1];
        let name = fields[fields.len() - 1]; // last field is the address
        entries.push(format!("- {} (pid:{} cmd:{})", name, pid, command));
    }

    if entries.is_empty() {
        return "(no listening ports detected)\n".to_string();
    }

    entries.sort();
    entries.dedup();
    entries.join("\n") + "\n"
}

/// Windows: use netstat.
fn discover_windows() -> String {
    let output = match Command::new("netstat").args(["-an", "-p", "TCP"]).output() {
        Ok(o) if o.status.success() => o,
        _ => return "(failed to run netstat)\n".to_string(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<String> = Vec::new();

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.contains("LISTENING") {
            let fields: Vec<&str> = trimmed.split_whitespace().collect();
            if fields.len() >= 2 {
                entries.push(format!("- {}", fields[1]));
            }
        }
    }

    if entries.is_empty() {
        return "(no listening ports detected)\n".to_string();
    }

    entries.sort();
    entries.dedup();
    entries.join("\n") + "\n"
}

/// Fallback: try ss, then netstat, then give up.
fn discover_fallback() -> String {
    // Try ss
    if let Ok(output) = Command::new("ss").args(["-tlnp"]).output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().skip(1).take(30).collect();
        if !lines.is_empty() {
            return lines.join("\n") + "\n";
        }
    }
    // Try netstat
    if let Ok(output) = Command::new("netstat").args(["-tlnp"]).output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout
            .lines()
            .filter(|l| l.contains("LISTEN"))
            .take(30)
            .collect();
        if !lines.is_empty() {
            return lines.join("\n") + "\n";
        }
    }
    "(no method available to detect listening ports)\n".to_string()
}
