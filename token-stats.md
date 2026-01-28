# Token Tracking Audit Report: Compliance & Data Integrity

**Auditor Role**: Financial Systems Consultant
**Objective**: Verify the sequential integrity and categorization accuracy of token tracking in Stakpak.

---

## 1. Sequential Data Flow (Audit Trail)

To verify a single token from API response to screen, the following sequence must be followed. This ensures no "leakage" or data corruption occurs between layers.

### Step 1: Ingestion (The API Response)
*   **File**: `libs/ai/src/types/response.rs`
*   **Component**: `struct Usage` and `struct InputTokenDetails`
*   **Logic**: Captures raw JSON from provider (Anthropic/OpenAI). 
*   **Accountant's Note**: This is the "Invoice Root." Accurate tracking starts here.

### Step 2: Transformation (The Normalizer)
*   **File**: `libs/shared/src/models/stakai_adapter.rs`
*   **Function**: `from_stakai_usage(&Usage)`
*   **Logic**: Maps raw provider fields (`no_cache`, `cache_read`, `cache_write`) to Stakpak's internal `LLMTokenUsage` struct.
*   **Accountant's Note**: Ensures unified accounting even if providers use different naming conventions.

### Step 3: Accumulation (The Ledger)
*   **File**: `tui/src/services/handlers/message.rs`
*   **Functions**: `handle_stream_usage` (real-time) and `handle_total_usage` (finalized totals).
*   **Logic**: Updates the session's running balance in the application state.
*   **Accountant's Note**: This is where individual message usage is aggregated into the "Session Total."

### Step 4: Storage (The Vault)
*   **File**: `tui/src/app.rs`
*   **Component**: `AppState::total_session_usage`
*   **Logic**: Holds the current balance of the session.

### Step 5: Reporting (The Financial Statement)
*   **File**: `tui/src/services/helper_block.rs` -> `push_usage_message`
*   **File**: `tui/src/services/side_panel.rs` -> `render_context_section`
*   **Logic**: Calculates percentages and tree-style breakdowns for the user.

---

## 2. Integrity Check: Input vs. Cache Categories

| Category | Technical Status | Accounting Definition |
| :--- | :--- | :--- |
| **Input Tokens** | `no_cache` | "Disposable Expense": Fresh tokens processed once and discarded (not saved to cache). |
| **Cache Write** | `cache_write` | "Capital Investment": First-time processing of data (Rulebooks/System prompts) stored for future reuse. |
| **Cache Read** | `cache_read` | "Asset Depreciation": Retrieve existing data at a 90% discount. No new processing required. |

**Financial Verification**: 
`Prompt Tokens` = `Input` + `Cache Write` + `Cache Read`. 
*If this sum does not match the provider's reported total, the audit fails.*

---

## 3. Findings & Observations
*   **Invisible Costs**: The "Cache Write" category often contains **Tool Definitions** (the code that allows the agent to read files or run bash). These are essential "Fixed Costs" charged on the first message of a session.
*   **Cache Breakpoints**: Tokens are only cached at specific intervals (typically every 2500 tokens for Anthropic). Usage that falls between these breakpoints may be reported as standard "Input."
*   **Efficiency Metric**: The system correctly identifies "Cache Read" as a high-value asset, significantly reducing the "Cost of Operation" for long conversations.
