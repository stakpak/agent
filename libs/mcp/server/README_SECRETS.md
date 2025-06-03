# Dynamic Secret Substitution

This document explains the dynamic secret substitution feature implemented in the MCP server tools.

## Overview

Dynamic secret substitution allows the LLM to reference and use redacted secret values using placeholders, without seeing the actual secret values. This provides a secure way to handle sensitive information while maintaining functionality.

## How It Works

### 1. Secret Detection and Redaction

When content containing secrets is processed (e.g., command output, file contents), the system:

1. **Detects secrets** using the gitleaks engine with comprehensive rules
2. **Generates placeholders** in the format `[REDACTED_SECRET:rule-id:short-hash]`
3. **Stores the mapping** between placeholders and actual values in a session file
4. **Returns redacted content** to the LLM with placeholders

Example:

```bash
# Original content
mysecret: api_125136713

# Shown to LLM
mysecret: [REDACTED_SECRET:api-key:bga278]
```

### 2. Secret Restoration

When the LLM provides commands or content that includes placeholders, the system:

1. **Identifies placeholders** in the input
2. **Looks up actual values** from the session mapping
3. **Restores original secrets** before execution
4. **Executes with real values** while keeping the LLM unaware

Example:

```bash
# LLM provides
echo "[REDACTED_SECRET:api-key:bga278]" > secret.txt

# System executes
echo "api_125136713" > secret.txt
```

## Session Management

### Session File

- **Location**: `.env.stakpak.session.secrets`
- **Format**: JSON object with placeholder-to-secret mappings
- **Persistence**: Maintained throughout the session
- **Security**: Local file with redaction mappings
- **Structure**: Pretty-printed JSON for readability

### File Format

```json
{
  "[REDACTED_SECRET:api-key:bga278]": "api_125136713",
  "[REDACTED_SECRET:token:def456]": "ghp_1234567890abcdef1234567890abcdef12345678",
  "[REDACTED_SECRET:aws-key:xyz789]": "AKIAIOSFODNN7EXAMPLE"
}
```

The file is automatically created when the first secret is detected and updated whenever new secrets are found. The JSON format makes it easy to inspect and debug the session mappings if needed.

## Implementation Details

### Tools Methods

#### `redact_and_store_secrets(content, path)`

- Detects secrets in content using gitleaks
- Generates placeholders for detected secrets
- Stores mappings in session file
- Returns redacted content

#### `restore_secrets_in_string(input)`

- Loads session redaction map
- Replaces placeholders with actual values
- Returns restored content

#### Session File Operations

- `load_session_redaction_map()`: Loads mappings from session file
- `save_session_redaction_map()`: Saves mappings to session file
- `add_to_session_redaction_map()`: Adds new mappings to existing session

### Tool Integration

All MCP tools that handle user input or produce output are integrated:

1. **`run_command`**: Restores secrets in commands before execution, redacts output
2. **`view`**: Redacts secrets in file contents shown to LLM
3. **`str_replace`**: Restores secrets in old/new strings before file operations
4. **`create`**: Restores secrets in file content before writing
5. **`insert`**: Restores secrets in inserted text before writing
6. **`generate_code`**: Redacts secrets in generated code shown to LLM

## Security Features

### Secret Detection

Uses comprehensive gitleaks rules to detect:

- API keys and tokens
- Database credentials
- Cloud provider keys (AWS, GCP, Azure)
- OAuth tokens
- Private keys and certificates
- And many more...

### Placeholder Generation

- **Unique identifiers**: Each secret gets a unique placeholder
- **Rule information**: Includes the detection rule for context
- **Short hash**: Prevents collisions while keeping placeholders readable
- **Consistent format**: `[REDACTED_SECRET:rule-id:hash]`

### Session Isolation

- **Local storage**: Session file stored locally
- **Session scope**: Each session can have different mappings
- **Cleanup**: Session files can be cleaned up after sessions end

## Usage Examples

### Example 1: Environment Variables

```bash
# LLM sees:
export API_KEY=[REDACTED_SECRET:generic-api-key:abc123]
export DB_PASSWORD=[REDACTED_SECRET:generic-password:def456]

# LLM can reference these in commands:
echo $API_KEY > /tmp/key.txt
curl -H "Authorization: Bearer $API_KEY" https://api.example.com

# System executes with real values:
echo actual_api_key_value > /tmp/key.txt
curl -H "Authorization: Bearer actual_api_key_value" https://api.example.com
```

**Session file (`.env.stakpak.session.secrets`):**

```json
{
  "[REDACTED_SECRET:generic-api-key:abc123]": "actual_api_key_value",
  "[REDACTED_SECRET:generic-password:def456]": "actual_db_password"
}
```

### Example 2: Configuration Files

```yaml
# LLM sees:
database:
  host: localhost
  password: [REDACTED_SECRET:password:xyz789]
# LLM can work with the structure while values remain secure
```

### Example 3: Command Output

```bash
# Command output contains secrets:
$ kubectl get secrets -o yaml

# LLM sees redacted version:
data:
  token: [REDACTED_SECRET:kubernetes-token:mno345]

# LLM can still work with the structure and reference the token
```

## Configuration

The feature is controlled by the `redact_secrets` flag in the MCP server configuration:

```rust
let tools = Tools::new(api_config, true); // Enable secret redaction
```

When disabled, all content passes through without modification.

## Limitations

1. **Session scope**: Mappings only persist within a single session
2. **Local storage**: Session files are stored locally (not distributed)
3. **Rule-based**: Detection depends on gitleaks rules (may miss custom formats)
4. **Performance**: Additional processing overhead for secret detection

## Security Considerations

1. **Session file protection**: Ensure `.env.stakpak.session.secrets` has appropriate permissions
2. **Cleanup**: Consider cleaning up session files after use
3. **Logging**: Avoid logging redacted content mappings
4. **Rule updates**: Keep gitleaks rules updated for comprehensive detection

## Testing

The implementation includes comprehensive tests:

- `test_session_redaction_map()`: Tests session file operations
- `test_redact_and_store_secrets()`: Tests end-to-end redaction and restoration

Run tests with:

```bash
cargo test test_session_redaction_map
cargo test test_redact_and_store_secrets
```
