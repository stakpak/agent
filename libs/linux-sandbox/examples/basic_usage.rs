use linux_sandbox::{Sandbox, SandboxPolicy};

fn main() {
    // Initialize logger
    env_logger::Builder::from_default_env().init();
    
    println!("=== Linux Sandbox Example ===\n");
    
    // Create default policy
    let policy = SandboxPolicy::default();
    
    println!("Policy configuration:");
    println!("  Mode: {:?}", policy.mode);
    println!("  Network allowed: {}", policy.network.allow_network);
    println!("  Command rules: {}", policy.network.command_rules.len());
    
    // Create sandbox
    let sandbox = Sandbox::new(policy.clone());
    
    println!("\n--- Testing safe command (git pull) ---");
    test_command(&sandbox, "git", &["--version"]);
    
    println!("\n--- Testing destructive command (rm -rf) ---");
    let full_command = "rm -rf /tmp/test";
    let is_destructive = policy.is_destructive(full_command);
    let allow_network = policy.should_allow_network(full_command);
    
    println!("Command: {}", full_command);
    println!("  Is destructive: {}", is_destructive);
    println!("  Allow network: {}", allow_network);
    
    // Note: We won't actually execute rm -rf, just show the policy decision
    println!("  Would execute: {}", !full_command.starts_with("rm"));
    
    println!("\n--- Audit Events ---");
    let events = sandbox.logger().get_events();
    println!("Total events logged: {}", events.len());
    
    for event in &events {
        println!("  - {:?}", event);
    }
    
    println!("\n=== Example Complete ===");
}

fn test_command(sandbox: &Sandbox, command: &str, args: &[&str]) {
    match sandbox.execute_command(command, args) {
        Ok(status) => {
            println!("Command executed successfully");
            println!("  Exit status: {:?}", status.code());
        }
        Err(e) => {
            println!("Command failed: {}", e);
        }
    }
}

