//! Benchmarks for the SecretManager redaction functionality
//!
//! This benchmark suite measures the performance of the secret redaction system,
//! including secret detection, redaction, and restoration operations.

use stakpak_shared::secret_manager::SecretManager;

fn main() {
    divan::main();
}

/// Helper to create test content with various secret patterns
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

    // Add some non-secret configuration
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

mod redaction_operations {
    use super::*;

    /// Benchmark redact_and_store_secrets with varying number of secrets
    #[divan::bench(args = [0, 1, 5, 10, 20])]
    fn redact_secrets(bencher: divan::Bencher, num_secrets: usize) {
        let manager = SecretManager::new(true, false);
        let content = generate_content_with_secrets(num_secrets);

        bencher
            .with_inputs(|| content.clone())
            .bench_local_values(|content| manager.redact_and_store_secrets(&content, None));
    }

    /// Benchmark redact_and_store_secrets with varying content sizes (no secrets)
    #[divan::bench(args = [10, 100, 500, 1000])]
    fn redact_varying_content_size(bencher: divan::Bencher, lines: usize) {
        let manager = SecretManager::new(true, false);
        let content = generate_content_without_secrets(lines);

        bencher
            .with_inputs(|| content.clone())
            .bench_local_values(|content| manager.redact_and_store_secrets(&content, None));
    }

    /// Benchmark restore_secrets_in_string
    #[divan::bench(args = [1, 5, 10, 20])]
    fn restore_secrets(bencher: divan::Bencher, num_secrets: usize) {
        let manager = SecretManager::new(true, false);
        let content = generate_content_with_secrets(num_secrets);

        // Redact the content first to populate the redaction map
        let redacted = manager.redact_and_store_secrets(&content, None);

        bencher
            .with_inputs(|| redacted.clone())
            .bench_local_values(|redacted| manager.restore_secrets_in_string(&redacted));
    }

    /// Benchmark redact_and_store_password
    #[divan::bench]
    fn redact_password(bencher: divan::Bencher) {
        let manager = SecretManager::new(true, false);
        let content = "Login with password supersecretpassword123 and continue".to_string();
        let password = "supersecretpassword123".to_string();

        bencher
            .with_inputs(|| (content.clone(), password.clone()))
            .bench_local_values(|(content, password)| {
                manager.redact_and_store_password(&content, &password)
            });
    }

    /// Benchmark with redaction disabled (passthrough mode)
    #[divan::bench(args = [0, 5, 10])]
    fn redact_disabled(bencher: divan::Bencher, num_secrets: usize) {
        let manager = SecretManager::new(false, false);
        let content = generate_content_with_secrets(num_secrets);

        bencher
            .with_inputs(|| content.clone())
            .bench_local_values(|content| manager.redact_and_store_secrets(&content, None));
    }
}

mod privacy_mode {
    use super::*;

    /// Benchmark with privacy mode enabled vs disabled
    #[divan::bench(consts = [false, true])]
    fn privacy_mode_comparison<const PRIVACY_MODE: bool>(bencher: divan::Bencher) {
        let manager = SecretManager::new(true, PRIVACY_MODE);
        let content = generate_content_with_secrets(5);

        bencher
            .with_inputs(|| content.clone())
            .bench_local_values(|content| manager.redact_and_store_secrets(&content, None));
    }
}

mod throughput {
    use super::*;

    /// Benchmark throughput with sequential operations
    #[divan::bench(args = [10, 50, 100])]
    fn sequential_redact_operations(bencher: divan::Bencher, num_operations: usize) {
        let manager = SecretManager::new(true, false);
        let content = generate_content_with_secrets(3);

        bencher.bench_local(|| {
            for _ in 0..num_operations {
                let _ = manager.redact_and_store_secrets(&content, None);
            }
        });
    }

    /// Benchmark sequential restore operations
    #[divan::bench(args = [10, 50, 100])]
    fn sequential_restore_operations(bencher: divan::Bencher, num_operations: usize) {
        let manager = SecretManager::new(true, false);
        let content = generate_content_with_secrets(3);

        // Redact first to populate the map
        let redacted = manager.redact_and_store_secrets(&content, None);

        bencher.bench_local(|| {
            for _ in 0..num_operations {
                let _ = manager.restore_secrets_in_string(&redacted);
            }
        });
    }
}
