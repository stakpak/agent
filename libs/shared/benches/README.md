# Benchmarks

Performance and memory profiling benchmarks for `stakpak-shared` library.

## Running Benchmarks

### Actor Model Memory Profiling
```bash
# All benchmarks
cargo bench -p stakpak-shared --bench actor_model_memory

# Specific groups
cargo bench -p stakpak-shared --bench actor_model_memory actor_initialization
cargo bench -p stakpak-shared --bench actor_model_memory async_operations
cargo bench -p stakpak-shared --bench actor_model_memory concurrent_operations
cargo bench -p stakpak-shared --bench actor_model_memory throughput
cargo bench -p stakpak-shared --bench actor_model_memory content_scaling
```

### Gitleaks Memory Profiling
```bash
# Full benchmark suite
cargo bench -p stakpak-shared --bench gitleaks_memory

# Specific categories
cargo bench -p stakpak-shared --bench gitleaks_memory config_initialization
cargo bench -p stakpak-shared --bench gitleaks_memory secret_detection
cargo bench -p stakpak-shared --bench gitleaks_memory real_world_scenarios
cargo bench -p stakpak-shared --bench gitleaks_memory batch_processing

