# Raw Benchmark Data Comparison

## 1. Redaction Operations - Varying Number of Secrets

### Actor Model (async)
```
async_operations::redact_async
├─ 1 secret:  130.7 µs  (median: 130.6 µs)
├─ 5 secrets: 557.8 µs  (median: 536.9 µs)
├─ 10 secrets: 916.6 µs (median: 891 µs)
└─ 20 secrets: 1.71 ms  (median: 1.676 ms)
```

### Direct Implementation (sync)
```
redaction_operations::redact_secrets
├─ 0 secrets:  40.7 µs   (median: 36.71 µs)
├─ 1 secret:   105.3 µs  (median: 101.8 µs)
├─ 5 secrets:  492.9 µs  (median: 484.5 µs)
├─ 10 secrets: 890.2 µs  (median: 873.4 µs)
└─ 20 secrets: 1.739 ms  (median: 1.678 ms)
```

---

## 2. Secret Restoration Operations

### Actor Model (async)
```
async_operations::restore_async
├─ 1 secret:   20.59 µs (median: 20.19 µs)
├─ 5 secrets:  20.97 µs (median: 20.24 µs)
├─ 10 secrets: 21.68 µs (median: 21.11 µs)
└─ 20 secrets: 23.7 µs  (median: 22.75 µs)
```

### Direct Implementation (sync)
```
redaction_operations::restore_secrets
├─ 1 secret:   9.173 µs  (median: 8.952 µs)
├─ 5 secrets:  10.03 µs  (median: 9.607 µs)
├─ 10 secrets: 11.13 µs  (median: 10.49 µs)
└─ 20 secrets: 12.99 µs  (median: 12.36 µs)
```

---

## 3. Password Redaction

### Actor Model (async)
```
async_operations::redact_password_async
Mean: 225.8 µs | Median: 220.1 µs
```

### Direct Implementation (sync)
```
redaction_operations::redact_password
Mean: 209.5 µs | Median: 160.4 µs
```

---

## 4. Privacy Mode Comparison

### Actor Model
```
async_operations::privacy_mode_async
├─ Privacy OFF: 544.7 µs (median: 530.5 µs)
└─ Privacy ON:  542.8 µs (median: 527.5 µs)
Overhead: ~0.3% (negligible)
```

### Direct Implementation
```
privacy_mode::privacy_mode_comparison
├─ Privacy OFF: 19.94 ms (median: 461.5 µs)
└─ Privacy ON:  20.33 ms (median: 431.4 µs)
Overhead: ~2% (negligible)
```

---

## 5. Concurrent Operations

### Actor Model Only
```
concurrent_operations::concurrent_redact (5 secrets)
├─ 2 tasks:  645.3 µs  (median: 616 µs)
├─ 4 tasks:  1.165 ms  (median: 1.118 ms)
├─ 8 tasks:  2.251 ms  (median: 2.172 ms)
└─ 16 tasks: 4.591 ms  (median: 4.424 ms)

concurrent_operations::concurrent_restore_async
├─ 2 tasks:  126.7 µs  (median: 122.3 µs)
├─ 4 tasks:  336.5 µs  (median: 324.4 µs)
├─ 8 tasks:  796.1 µs  (median: 767.1 µs)
└─ 16 tasks: 2.523 ms  (median: 2.326 ms)

concurrent_operations::mixed_workload
├─ 4 tasks:  1.269 ms  (median: 1.224 ms)
├─ 8 tasks:  2.512 ms  (median: 2.429 ms)
└─ 16 tasks: 5.01 ms   (median: 4.835 ms)
```

---

## 6. Sequential Throughput Tests

### Redaction Throughput (3 secrets per operation)

**Actor Model:**
```
throughput::sequential_redact_throughput
├─ 10 ops:  24.44 ms (median: 4.798 ms)  [~205 ops/sec median, ~41 ops/sec mean]
├─ 50 ops:  44.42 ms (median: 24.25 ms) [~2,062 ops/sec median, ~1,126 ops/sec mean]
└─ 100 ops: 67.57 ms (median: 48.98 ms) [~2,042 ops/sec median, ~1,480 ops/sec mean]
```

**Direct Implementation:**
```
throughput::sequential_redact_operations
├─ 10 ops:  4.135 ms (median: 3.703 ms)  [~2,700 ops/sec median, ~2,418 ops/sec mean]
├─ 50 ops:  18.79 ms (median: 18.44 ms) [~2,712 ops/sec median, ~2,661 ops/sec mean]
└─ 100 ops: 38.22 ms (median: 37.91 ms) [~2,637 ops/sec median, ~2,616 ops/sec mean]
```

### Restoration Throughput

**Actor Model:**
```
throughput::sequential_restore_async_throughput
├─ 10 ops:  319.8 µs (median: 341 µs)    [~29,325 ops/sec median, ~31,271 ops/sec mean]
├─ 50 ops:  1.715 ms (median: 1.767 ms)  [~28,298 ops/sec median, ~29,155 ops/sec mean]
└─ 100 ops: 3.39 ms  (median: 3.505 ms)  [~28,531 ops/sec median, ~29,498 ops/sec mean]
```

**Direct Implementation:**
```
throughput::sequential_restore_operations
├─ 10 ops:  94.81 µs (median: 92.47 µs)  [~108,144 ops/sec median, ~105,474 ops/sec mean]
├─ 50 ops:  485 µs   (median: 463.1 µs)  [~107,984 ops/sec median, ~103,093 ops/sec mean]
└─ 100 ops: 956 µs   (median: 912.1 µs)  [~109,635 ops/sec median, ~104,603 ops/sec mean]
```

---

## 7. Content Size Scaling (No Secrets)

### Actor Model
```
content_scaling::redact_varying_size
├─ 10 lines:   28.82 µs (median: 28.42 µs)
├─ 100 lines:  30.17 µs (median: 29.84 µs)
├─ 500 lines:  47.29 µs (median: 45.09 µs)
└─ 1000 lines: 58.61 µs (median: 55.44 µs)
```

### Direct Implementation
```
redaction_operations::redact_varying_content_size
├─ 10 lines:   26 µs     (median: 25.07 µs)
├─ 100 lines:  83.3 µs   (median: 79.82 µs)
├─ 500 lines:  306.5 µs  (median: 302.4 µs)
└─ 1000 lines: 579.4 µs  (median: 578.1 µs)
```

---

## 8. Actor Initialization Overhead

**Only applicable to Actor Model:**
```
actor_initialization
├─ launch_default_actor:     8.171 µs  (median: 4.824 µs)
├─ launch_privacy_actor:     5.237 µs  (median: 4.334 µs)
├─ launch_no_redaction:      5.309 µs  (median: 4.579 µs)
└─ launch_with_privacy:
   ├─ false:                 5.823 µs  (median: 4.319 µs)
   └─ true:                  5.261 µs  (median: 4.329 µs)

Memory per actor:
- Max allocation:    4.2 KB
- Total allocation:  4.947 KB
- Deallocation:      655 B
- Net memory:        ~4.3 KB per actor
```
