//! Memory profiling benchmarks for SecretManager Actor Model
//!
//! This benchmark suite measures memory allocations and performance of the
//! actor-based SecretManager implementation, including:
//! - Actor initialization overhead
//! - Message passing memory costs
//! - Async operation allocations
//! - Concurrent operation scaling
//! - Channel capacity impact
//!
//! Run with: cargo bench --bench actor_model_memory

use divan::AllocProfiler;
use stakpak_shared::secret_manager;
use tokio::runtime::Runtime;

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    divan::main();
}

/// Helper to create test content with various secret patterns
fn generate_content_with_secrets(num_secrets: usize) -> String {
    let mut content = String::from("# Configuration file\n\n");

    for i in 0..num_secrets {
        match i % 5 {
            0 => content.push_str(&format!(
                "export AWS_ACCESS_KEY_ID_{i}=AKIAIOSFODNN7EXAMPLE{i:04}\n"
            )),
            1 => content.push_str(&format!(
                "export GITHUB_TOKEN_{i}=ghp_1234567890abcdef1234567890abcde{i:05}\n"
            )),
            2 => content.push_str(&format!(
                "export API_KEY_{i}=Kx9mP2nQ8rT4vW7yZ3cF6hJ1lN5sA0bD{i:04}\n"
            )),
            3 => content.push_str(&format!(
                "export SECRET_TOKEN_{i}=xy9mP2nQ8rT4vW7yZ3cF6hJ1lN5sAdef{i:04}\n"
            )),
            4 => content.push_str(&format!(
                "export PRIVATE_KEY_{i}=sk-proj-abcdefghijklmnopqrstuvwxyz12{i:04}\n"
            )),
            _ => unreachable!(),
        }
    }

    content.push_str("\n# Non-secret configuration\n");
    content.push_str("export DEBUG=true\n");
    content.push_str("export PORT=3000\n");
    content
}

/// Helper to generate content without secrets
fn generate_content_without_secrets(lines: usize) -> String {
    let mut content = String::from("# Configuration file\n\n");
    for i in 0..lines {
        content.push_str(&format!("export CONFIG_VALUE_{i}=some_value_{i}\n"));
    }
    content
}

// ============================================================================
// ACTOR INITIALIZATION BENCHMARKS
// ============================================================================

mod actor_initialization {
    use super::*;

    /// Measure memory allocated during actor launch (default mode)
    #[divan::bench]
    fn launch_default_actor(bencher: divan::Bencher) {
        // Keep runtime alive outside the benchmark to prevent panic
        let rt = Runtime::new().unwrap();
        bencher.bench_local(|| {
            let _guard = rt.enter();
            let _handle =
                divan::black_box(secret_manager::launch_secret_manager(true, false, None));
            // Ensure actor finishes initialization by performing a dummy operation
            drop(_handle);
        });
    }

    /// Measure memory allocated during actor launch (privacy mode)
    #[divan::bench]
    fn launch_privacy_actor(bencher: divan::Bencher) {
        let rt = Runtime::new().unwrap();
        bencher.bench_local(|| {
            let _guard = rt.enter();
            let _handle = divan::black_box(secret_manager::launch_secret_manager(true, true, None));
            drop(_handle);
        });
    }

    /// Measure memory allocated during actor launch (redaction disabled)
    #[divan::bench]
    fn launch_no_redaction_actor(bencher: divan::Bencher) {
        let rt = Runtime::new().unwrap();
        bencher.bench_local(|| {
            let _guard = rt.enter();
            let _handle =
                divan::black_box(secret_manager::launch_secret_manager(false, false, None));
            drop(_handle);
        });
    }

    /// Compare initialization modes
    #[divan::bench(consts = [false, true])]
    fn launch_with_privacy<const PRIVACY_MODE: bool>(bencher: divan::Bencher) {
        let rt = Runtime::new().unwrap();
        bencher.bench_local(|| {
            let _guard = rt.enter();
            let _handle = divan::black_box(secret_manager::launch_secret_manager(
                true,
                PRIVACY_MODE,
                None,
            ));
            drop(_handle);
        });
    }
}

// ============================================================================
// ASYNC OPERATIONS BENCHMARKS
// ============================================================================

mod async_operations {
    use super::*;

    /// Benchmark async redaction with varying numbers of secrets
    #[divan::bench(args = [0, 1, 5, 10, 20])]
    fn redact_async(bencher: divan::Bencher, num_secrets: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(num_secrets);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| {
                rt.block_on(async {
                    divan::black_box(
                        handle
                            .redact_and_store_secrets(&content, None)
                            .await
                            .unwrap(),
                    )
                })
            });
    }

    /// Benchmark async restoration with varying numbers of secrets
    #[divan::bench(args = [1, 5, 10, 20])]
    fn restore_async(bencher: divan::Bencher, num_secrets: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(num_secrets);

        // Redact first to populate the map
        let redacted = rt.block_on(async {
            handle
                .redact_and_store_secrets(&content, None)
                .await
                .unwrap()
        });

        bencher
            .with_inputs(|| redacted.clone())
            .bench_values(|redacted| {
                rt.block_on(async {
                    divan::black_box(handle.restore_secrets_in_string(&redacted).await.unwrap())
                })
            });
    }

    /// Benchmark async password redaction
    #[divan::bench]
    fn redact_password_async(bencher: divan::Bencher) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let password = "supersecretpassword123";

        bencher.bench(|| {
            rt.block_on(async {
                divan::black_box(handle.redact_and_store_password(password).await.unwrap())
            })
        });
    }

    /// Benchmark privacy mode overhead
    #[divan::bench(consts = [false, true])]
    fn privacy_mode_async<const PRIVACY_MODE: bool>(bencher: divan::Bencher) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, PRIVACY_MODE, None);
        let content = generate_content_with_secrets(5);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| {
                rt.block_on(async {
                    divan::black_box(
                        handle
                            .redact_and_store_secrets(&content, None)
                            .await
                            .unwrap(),
                    )
                })
            });
    }

    /// Benchmark with redaction disabled (passthrough mode)
    #[divan::bench(args = [0, 5, 10])]
    fn redact_disabled(bencher: divan::Bencher, num_secrets: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(false, false, None);
        let content = generate_content_with_secrets(num_secrets);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| {
                rt.block_on(async {
                    divan::black_box(
                        handle
                            .redact_and_store_secrets(&content, None)
                            .await
                            .unwrap(),
                    )
                })
            });
    }
}

// ============================================================================
// CONCURRENT OPERATIONS BENCHMARKS
// ============================================================================

mod concurrent_operations {
    use super::*;

    /// Benchmark concurrent redaction operations
    #[divan::bench(args = [2, 4, 8, 16])]
    fn concurrent_redact(bencher: divan::Bencher, num_tasks: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(5);

        bencher.bench(|| {
            rt.block_on(async {
                let tasks: Vec<_> = (0..num_tasks)
                    .map(|_| {
                        let h = handle.clone();
                        let c = content.clone();
                        tokio::spawn(
                            async move { h.redact_and_store_secrets(&c, None).await.unwrap() },
                        )
                    })
                    .collect();

                for task in tasks {
                    let _ = divan::black_box(task.await.unwrap());
                }
            });
        });
    }

    /// Benchmark concurrent restoration (async through actor)
    #[divan::bench(args = [2, 4, 8, 16])]
    fn concurrent_restore_async(bencher: divan::Bencher, num_tasks: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(5);

        let redacted = rt.block_on(async {
            handle
                .redact_and_store_secrets(&content, None)
                .await
                .unwrap()
        });

        bencher.bench(|| {
            rt.block_on(async {
                let tasks: Vec<_> = (0..num_tasks)
                    .map(|_| {
                        let h = handle.clone();
                        let r = redacted.clone();
                        tokio::spawn(async move { h.restore_secrets_in_string(&r).await.unwrap() })
                    })
                    .collect();

                for task in tasks {
                    let _ = divan::black_box(task.await.unwrap());
                }
            });
        });
    }

    /// Benchmark mixed read/write workload
    #[divan::bench(args = [4, 8, 16])]
    fn mixed_workload(bencher: divan::Bencher, num_tasks: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(5);

        let redacted = rt.block_on(async {
            handle
                .redact_and_store_secrets(&content, None)
                .await
                .unwrap()
        });

        bencher.bench(|| {
            rt.block_on(async {
                let tasks: Vec<_> = (0..num_tasks)
                    .map(|i| {
                        let h = handle.clone();
                        let c = content.clone();
                        let r = redacted.clone();
                        tokio::spawn(async move {
                            if i % 2 == 0 {
                                // Write operation
                                h.redact_and_store_secrets(&c, None).await.unwrap()
                            } else {
                                // Read operation (through actor)
                                h.restore_secrets_in_string(&r).await.unwrap()
                            }
                        })
                    })
                    .collect();

                for task in tasks {
                    let _ = divan::black_box(task.await.unwrap());
                }
            });
        });
    }
}

// ============================================================================
// THROUGHPUT BENCHMARKS
// ============================================================================

mod throughput {
    use super::*;

    /// Benchmark sequential redaction throughput
    #[divan::bench(args = [10, 50, 100])]
    fn sequential_redact_throughput(bencher: divan::Bencher, num_operations: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(3);

        bencher.bench(|| {
            rt.block_on(async {
                for _ in 0..num_operations {
                    let _ = divan::black_box(
                        handle
                            .redact_and_store_secrets(&content, None)
                            .await
                            .unwrap(),
                    );
                }
            });
        });
    }

    /// Benchmark sequential restoration throughput (async through actor)
    #[divan::bench(args = [10, 50, 100])]
    fn sequential_restore_async_throughput(bencher: divan::Bencher, num_operations: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_with_secrets(3);

        let redacted = rt.block_on(async {
            handle
                .redact_and_store_secrets(&content, None)
                .await
                .unwrap()
        });

        bencher.bench(|| {
            rt.block_on(async {
                for _ in 0..num_operations {
                    let _ = divan::black_box(
                        handle.restore_secrets_in_string(&redacted).await.unwrap(),
                    );
                }
            });
        });
    }
}

// ============================================================================
// CONTENT SIZE SCALING BENCHMARKS
// ============================================================================

mod content_scaling {
    use super::*;

    /// Benchmark redaction with varying content sizes
    #[divan::bench(args = [10, 100, 500, 1000])]
    fn redact_varying_size(bencher: divan::Bencher, lines: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_without_secrets(lines);

        bencher
            .with_inputs(|| content.clone())
            .bench_values(|content| {
                rt.block_on(async {
                    divan::black_box(
                        handle
                            .redact_and_store_secrets(&content, None)
                            .await
                            .unwrap(),
                    )
                })
            });
    }

    /// Benchmark async restoration with varying content sizes
    #[divan::bench(args = [10, 100, 500, 1000])]
    fn restore_async_varying_size(bencher: divan::Bencher, lines: usize) {
        let rt = Runtime::new().unwrap();
        let _guard = rt.enter();
        let handle = secret_manager::launch_secret_manager(true, false, None);
        let content = generate_content_without_secrets(lines);

        let redacted = rt.block_on(async {
            handle
                .redact_and_store_secrets(&content, None)
                .await
                .unwrap()
        });

        bencher
            .with_inputs(|| redacted.clone())
            .bench_values(|redacted| {
                rt.block_on(async {
                    divan::black_box(handle.restore_secrets_in_string(&redacted).await.unwrap())
                })
            });
    }
}
