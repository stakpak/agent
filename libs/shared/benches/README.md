# Benchmarks

Performance and memory profiling benchmarks for `stakpak-shared` library.

## Running Benchmarks

### Secret Manager Performance
```bash
# All benchmarks
cargo bench -p stakpak-shared --bench secret_manager

# Specific groups
cargo bench -p stakpak-shared --bench secret_manager redaction_operations
cargo bench -p stakpak-shared --bench secret_manager privacy_mode
cargo bench -p stakpak-shared --bench secret_manager throughput
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

