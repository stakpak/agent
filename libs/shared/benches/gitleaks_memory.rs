//! Memory profiling benchmarks for Gitleaks secret detection
//!
//! This benchmark suite measures memory allocations during:
//! - Initial loading of gitleaks configuration and rules
//! - Secret detection operations
//! - Different privacy modes
//! - Various content sizes
//!
//! Run with: cargo bench --bench gitleaks_memory

use divan::AllocProfiler;
use stakpak_shared::secrets::gitleaks::{create_gitleaks_config, detect_secrets};

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    divan::main();
}

/// Helper to generate test content with various secret patterns
fn generate_content_with_secrets(num_secrets: usize) -> String {
    let mut content = String::from("# Configuration file\n\n");

    for i in 0..num_secrets {
        match i % 5 {
            0 => {
                // AWS-style key
                content.push_str(&format!(
                    "export AWS_ACCESS_KEY_ID_{i}=AKIAIOSFODNN7EXAMPLE{i:04}\n"
                ));
            }
            1 => {
                // GitHub token
                content.push_str(&format!(
                    "export GITHUB_TOKEN_{i}=ghp_1234567890abcdef1234567890abcde{i:05}\n"
                ));
            }
            2 => {
                // Generic API key with high entropy
                content.push_str(&format!(
                    "export API_KEY_{i}=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD{i:04}\n"
                ));
            }
            3 => {
                // Secret token
                content.push_str(&format!(
                    "export SECRET_TOKEN_{i}=xy9mP2nQ8rT4vW7yZ3cF6hJ1lN5sAdef{i:04}\n"
                ));
            }
            4 => {
                // Private key pattern
                content.push_str(&format!(
                    "export PRIVATE_KEY_{i}=sk-proj-abcdefghijklmnopqrstuvwxyz12{i:04}\n"
                ));
            }
            _ => unreachable!(),
        }
    }

    content.push_str("\n# Non-secret configuration\n");
    content.push_str("export DEBUG=true\n");
    content.push_str("export PORT=3000\n");
    content.push_str("export LOG_LEVEL=info\n");

    content
}

/// Helper to generate content without secrets (for baseline)
fn generate_content_without_secrets(lines: usize) -> String {
    let mut content = String::from("# Configuration file\n\n");

    for i in 0..lines {
        content.push_str(&format!("export CONFIG_VALUE_{i}=some_value_{i}\n"));
    }

    content
}

mod config_initialization {
    use super::*;

    /// Measure memory allocated when creating the default gitleaks config.
    #[divan::bench]
    fn load_default_config() {
        let config = divan::black_box(create_gitleaks_config(false));
        drop(config);
    }

    /// Measure memory allocated when creating gitleaks config with privacy rules.
    #[divan::bench]
    fn load_privacy_config() {
        let config = divan::black_box(create_gitleaks_config(true));
        drop(config);
    }

    /// Count the number of rules in the default config
    #[divan::bench]
    fn count_default_rules() -> usize {
        let config = create_gitleaks_config(false);
        config.rules.len()
    }

    /// Count the number of rules in the privacy config
    #[divan::bench]
    fn count_privacy_rules() -> usize {
        let config = create_gitleaks_config(true);
        config.rules.len()
    }
}

mod secret_detection {
    use super::*;

    /// Benchmark memory allocations during secret detection with varying numbers of secrets
    #[divan::bench(args = [1, 5, 10, 20, 50])]
    fn detect_with_secrets(bencher: divan::Bencher, num_secrets: usize) {
        let content = generate_content_with_secrets(num_secrets);
        let config = create_gitleaks_config(false);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| divan::black_box(detect_secrets(&content, None, &config)));
    }

    /// Benchmark memory allocations during secret detection with varying content sizes (no secrets)
    #[divan::bench(args = [10, 100, 500, 1000])]
    fn detect_varying_content_size(bencher: divan::Bencher, lines: usize) {
        let content = generate_content_without_secrets(lines);
        let config = create_gitleaks_config(false);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| divan::black_box(detect_secrets(&content, None, &config)));
    }

    /// Benchmark memory with privacy mode enabled
    #[divan::bench(args = [1, 5, 10])]
    fn detect_with_privacy_mode(bencher: divan::Bencher, num_secrets: usize) {
        let content = generate_content_with_secrets(num_secrets);
        let config = create_gitleaks_config(true);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| divan::black_box(detect_secrets(&content, None, &config)));
    }

    /// Compare privacy mode on vs off
    #[divan::bench(consts = [false, true])]
    fn privacy_mode_comparison<const PRIVACY_MODE: bool>(bencher: divan::Bencher) {
        let content = generate_content_with_secrets(5);
        let config = create_gitleaks_config(PRIVACY_MODE);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| divan::black_box(detect_secrets(&content, None, &config)));
    }
}

mod real_world_scenarios {
    use super::*;

    /// Simulate scanning a typical configuration file
    #[divan::bench]
    fn scan_typical_config_file(bencher: divan::Bencher) {
        let content = r#"
# Database Configuration
DB_HOST=localhost
DB_PORT=5432
DB_NAME=myapp
DB_USER=admin
DB_PASSWORD=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD

# AWS Configuration
AWS_REGION=us-east-1
AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE
AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY

# GitHub Token
GITHUB_TOKEN=ghp_1234567890abcdef1234567890abcdef12

# Application Settings
APP_NAME=MyApp
APP_ENV=production
LOG_LEVEL=info
"#;
        let config = create_gitleaks_config(false);

        bencher.bench(|| divan::black_box(detect_secrets(content, Some("config.env"), &config)));
    }

    /// Simulate scanning a large log file
    #[divan::bench]
    fn scan_large_log_file(bencher: divan::Bencher) {
        let mut content = String::new();

        // Simulate 1000 lines of logs with occasional secrets
        for i in 0..1000 {
            if i % 100 == 0 {
                // Every 100th line has a secret
                content.push_str(&format!(
                    "[{}] INFO: User authenticated with token: ghp_1234567890abcdef1234567890abcde{i:05}\n",
                    i
                ));
            } else {
                content.push_str(&format!(
                    "[{}] INFO: Processing request from user_{}\n",
                    i, i
                ));
            }
        }
        let config = create_gitleaks_config(false);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| {
                divan::black_box(detect_secrets(&content, Some("app.log"), &config))
            });
    }

    /// Simulate scanning source code with embedded secrets
    #[divan::bench]
    fn scan_source_code(bencher: divan::Bencher) {
        let content = r#"
package main

import "fmt"

const (
    // Don't commit this!
    apiKey = "Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD"

    // Production database
    dbUrl = "postgresql://admin:SuperSecret123@db.example.com/prod"
)

func main() {
    // AWS credentials
    awsKey := "AKIAIOSFODNN7EXAMPLE"
    awsSecret := "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"

    fmt.Println("Starting application...")
}
"#;
        let config = create_gitleaks_config(false);

        bencher.bench(|| divan::black_box(detect_secrets(content, Some("main.go"), &config)));
    }
}

mod batch_processing {
    use super::*;

    /// Simulate scanning multiple files in sequence
    #[divan::bench(args = [5, 10, 20])]
    fn scan_multiple_files(bencher: divan::Bencher, num_files: usize) {
        let files: Vec<String> = (0..num_files)
            .map(|i| generate_content_with_secrets(i + 1))
            .collect();
        let config = create_gitleaks_config(false);

        bencher.with_inputs(|| files.clone()).bench_values(|files| {
            let mut total_secrets = 0;
            for (i, content) in files.iter().enumerate() {
                let secrets = detect_secrets(content, Some(&format!("file_{}.txt", i)), &config);
                total_secrets += secrets.len();
            }
            divan::black_box(total_secrets)
        });
    }

    /// Simulate repeated scans of the same content (cache behavior)
    #[divan::bench(args = [10, 50, 100])]
    fn repeated_scans(bencher: divan::Bencher, num_scans: usize) {
        let content = generate_content_with_secrets(5);
        let config = create_gitleaks_config(false);

        bencher.bench(|| {
            let mut total_secrets = 0;
            for _ in 0..num_scans {
                let secrets = detect_secrets(&content, None, &config);
                total_secrets += secrets.len();
            }
            divan::black_box(total_secrets)
        });
    }
}
