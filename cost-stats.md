# Cost Tracking Audit Report: Financial Logic & Tiered Pricing

**Auditor Role**: Financial Systems Consultant
**Objective**: Validate the pricing logic, tiered thresholding, and cost-allocation accuracy in Stakpak.

---

## 1. The Pricing Engine (Core Logic)

Stakpak does not use a flat rate. It implements a multi-tiered pricing model to account for context-window inflation and cache discounts.

### Logic Component: `libs/shared/src/models/model_pricing.rs`
*   **Struct**: `ContextPricingTier`
*   **Control Field**: `upper_bound`
*   **Financial Impact**: As the conversation grows (e.g., past 200k tokens), the system automatically selects a more expensive tier to reflect provider surcharges.

### Tier Thresholds (Claude 3.5 Sonnet Example)
Referencing `libs/shared/src/models/integrations/anthropic.rs`:

| Tier | Condition | Input Cost (M) | Output Cost (M) |
| :--- | :--- | :--- | :--- |
| **Tier 1 (Base)** | < 200,000 tokens | $3.00 | $15.00 |
| **Tier 2 (Heavy)** | > 200,000 tokens | $6.00 | $22.50 |

---

## 2. Multiplier Verification (The "Cache" Discount)

The system applies the following multipliers to the "Base Input Cost" of the active tier:

1.  **Standard Input**: `1.0x` Base Rate.
2.  **Cache Write**: `1.25x` Base Rate. (Reflects the one-time cost of indexing and storing the context).
3.  **Cache Read**: `0.10x` Base Rate. (**90% Discount**). This is the primary driver of session ROI.

---

## 3. Financial Reconciliation (Sample Audit)

**Scenario**: A 63,947 token session (Under 200k threshold = Tier 1 pricing).

| Component | Count | Rate | Calculation | Subtotal |
| :--- | :--- | :--- | :--- | :--- |
| **Pure Input** | 16,804 | $3.00 | `(16,804/1M) * 3` | $0.0504 |
| **Cache Write** | 11,445 | $3.75 | `(11,445/1M) * 3.75` | $0.0429 |
| **Cache Read** | 34,335 | $0.30 | `(34,335/1M) * 0.3` | $0.0103 |
| **Completion** | 1,363 | $15.00 | `(1,363/1M) * 15` | $0.0205 |
| **TOTAL** | **63,947** | - | - | **$0.1241** |

---

## 4. Audit Conclusion: Systems Integrity

### Strengths:
*   **Granular Reporting**: By separating "Input" from "Cache Write," the system avoids overcharging or under-calculating the storage overhead.
*   **Dynamic Tiering**: The code in `model_pricing.rs` successfully implements `upper_bound` logic, ensuring that users are charged accurately as context windows expand.
*   **Adapter Neutrality**: The adapter layer (`stakai_adapter.rs`) ensures that even if a provider changes their JSON structure, the internal "Financial Ledger" remains consistent.

### Audit Note for the CFO:
The "Total tokens" number is a **volumetric** metric, but the "Total Cost" is a **weighted** metric. The efficiency of the session is measured by the ratio of `Cache Read / Total Prompt`. In this session, the efficiency is **54.8%**, indicating highly effective use of the caching system.
