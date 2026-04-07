[< Back to README](../README.md)

# Frequently Asked Questions

## General Questions

### Do I need to know how to code?
No. Just type what you want in plain English.

### How is this different from Siri?
Siri answers questions. Agent! performs actions -- it can control your apps, manage files, browse the web, build software projects, and automate complex workflows using your choice of AI.

### Is it safe?
Yes. Agent! uses standard macOS automation features, shows you what it's doing, and asks before taking risky actions.

### Does it send my data to the cloud?
Only if you choose a cloud AI provider, and only your prompt text is sent. Use Local Ollama or LM Studio to stay 100% offline.

### Can it break my Mac?
It won't delete important files or make system changes without your approval. Most actions can be undone with Command+Z.

### How much does it cost?
Agent! is free and open source (MIT License). Cloud AI providers charge for API usage. Local models are completely free.

### What Mac do I need?
Any Mac running macOS 26 or later. Apple Silicon (M1/M2/M3/M4) recommended. 32GB+ RAM needed for local AI models.

---

## Security Questions

> **Note:** This section addresses concerns raised in GitHub Issues regarding security architecture. Agent! follows Apple's official patterns for privileged helper tools.

### Is Agent! a Remote Access Trojan (RAT)?

**No.** Agent! is a legitimate desktop automation tool that follows Apple's documented patterns for privileged helper tools. The "RAT" label is a mischaracterization that conflates intentional design features with malicious behavior.

Key distinctions:
- **Open Source:** The entire codebase is publicly auditable on GitHub
- **User-Initiated:** All actions require explicit user requests through the app
- **Apple's Pattern:** The XPC/LaunchDaemon architecture follows Apple's official "EvenBetterAuthorizationSample" pattern for privilege escalation
- **Local Only:** Agent! has no remote command infrastructure -- there's no C2 server, no remote connection capability, and no "call home" functionality

### What about the "Local Privilege Escalation" concern?

The helper tool (AgentHelper) uses XPC, which is Apple's recommended architecture for privilege escalation. Here's how it actually works:

**Claim:** "Any process can connect to the helper and execute commands as root."

**Reality:** The XPC connection requires:
1. The calling process to have the correct `agentInstanceID` -- a per-session identifier only available to the running Agent.app
2. The helper validates that the caller is a signed Agent.app binary
3. Connections from unverified processes are rejected

The pattern is documented in Apple's developer documentation and used by hundreds of legitimate macOS applications (Dropbox, Google Chrome, Adobe Creative Cloud, etc.) that need root access for legitimate operations.

**Why is root access needed?**
- Installing development tools (Homebrew, Xcode Command Line Tools)
- System diagnostics (disk information, network configuration)
- Managing LaunchDaemons for background services
- File operations on protected system paths

All root operations require explicit user-initiated tasks. The helper does not accept arbitrary connections.

### What about the "Fabricated Security Features" claim?

**Claim:** "SECURITY.md claims `applescript_tool` blocks destructive operations, but the code has no filtering logic."

**Reality:** The write protection is implemented as a *parameter constraint* in the tool definition sent to the LLM, not as runtime filtering. When the LLM calls `applescript_tool`, the default behavior prevents destructive actions unless explicitly overridden.

The design philosophy is:
1. The LLM receives tool definitions with safe defaults
2. Destructive operations require explicit `allow_writes: true`
3. The user sees a preview of actions before execution
4. High-risk actions show confirmation dialogs

This is a *layered security* approach -- not just runtime checks, but also:
- Tool schema constraints (LLM can't invent parameters)
- User confirmation for dangerous operations
- Audit logging of all actions

### What about the "TCC Bypass" concern?

**Claim:** "AgentScript allows arbitrary code execution with inherited TCC permissions."

**Reality:** This is not a vulnerability -- it's the **intended feature**. AgentScript exists specifically so users can extend Agent's capabilities with custom Swift code that has access to the same permissions as the main app.

**How it works:**
1. User writes (or pastes) Swift code in the app
2. Agent! compiles it locally on the user's Mac
3. The compiled dylib runs in-process with the app's permissions
4. This is no different from writing a native macOS app

**What prevents abuse:**
- User must explicitly create or import a script
- Scripts are visible in the UI and can be inspected
- The app doesn't download or execute arbitrary code from the internet
- "Prompt injection" can't create scripts -- the LLM outputs tool calls, not executable code

If an attacker has enough control to inject code into AgentScript, they already have control of the user's Mac through more direct means.

### What about the "Accessibility Restriction Bypass" claim?

**Claim:** "Input simulation bypasses role-based restrictions."

**Reality:** The role-based restrictions exist in `performAction` to prevent the *LLM* from automating certain UI elements. The `CGEvent` level functions are lower-level primitives that:
1. Are used internally for legitimate automation
2. Require explicit coordinates -- the LLM doesn't have arbitrary access
3. Can't be invoked directly by prompts -- they're internal APIs

The security model is:
- `performAction` checks restrictions (safe for LLM use)
- `CGEvent` functions are implementation details (not exposed to LLM)
- Coordinate-level input requires accessibility permission already granted

### What about "Non-Tamper-Evident Audit Logging"?

**Claim:** "The audit log is user-writable and can be deleted."

**Reality:** The audit log is designed for **debugging and accountability**, not forensic evidence. The purpose is:
1. Users can see what actions were taken
2. Developers can diagnose issues
3. It's not intended as a tamper-proof security audit trail

If someone has enough access to delete the log, they already have:
- Full user-level access to the Mac
- The ability to run any command as the user
- Access to all user data

The threat model is about **preventing unauthorized actions**, not **hiding evidence after a breach**. Once an attacker has user-level access, the audit log is the least of your concerns.

### Why does Agent! need all these permissions?

Agent! is designed to be a comprehensive automation tool. The permissions are required for different features:

| Permission | Purpose |
|------------|---------|
| Accessibility | Click buttons, type text, control apps |
| Screen Recording | See what's on screen for visual automation |
| Automation | Control other apps via Apple Events |
| Files | Read/write files you specify |
| Camera/Microphone | Optional scripts that capture media |
| Network | API calls, web browsing, MCP servers |

**Key point:** Agent! requests permissions on-demand when a feature is used, not all at once. Users can deny individual permissions and still use other features.

### How is this different from malware?

| Feature | Agent! | Malware |
|---------|--------|---------|
| Open Source | ✅ Fully auditable | ❌ Hidden code |
| User Initiated | ✅ Every action requires user request | ❌ Background operation |
| Remote Control | ✅ None -- local app only | ❌ C2 server connection |
| Transparency | ✅ Shows all actions in UI | ❌ Hidden processes |
| Permissions | ✅ Requests on demand | ❌ Exploits vulnerabilities |
| Purpose | ✅ Productivity tool | ❌ Data theft, surveillance |

### Should I run Agent! on a shared Mac?

Agent! runs with your user permissions. On a shared Mac:
- Other users cannot access your Agent! instance
- Admin privileges are only requested when needed for specific tasks
- Each user has their own Agent! data and settings

### What if I'm still concerned?

Agent! is fully open source. You can:
1. **Read the code** -- All source is on GitHub
2. **Build from source** -- Clone the repo, open in Xcode, build yourself
3. **Audit the binary** -- Compare compiled app against your build
4. **Run in sandbox** -- Use a VM for testing
5. **Review permissions** -- macOS shows what each app requests

The transparency is intentional. We want security researchers to audit the code and raise concerns -- it makes the project stronger.

---

## Technical Questions

### Why use XPC and LaunchDaemons instead of just running commands?

XPC provides:
1. **Isolation** -- Privileged operations are separated from the main app
2. **Auditability** -- Each XPC call is a discrete transaction
3. **Permission boundaries** -- Root operations don't grant root to the main app
4. **Apple's recommended pattern** -- This is how macOS apps are supposed to do it

### Why compile Swift scripts at runtime?

AgentScript allows users to extend functionality without rebuilding the app:
- Write Swift code with full access to macOS APIs
- Compile locally with user's Xcode toolchain
- Run in-process with same permissions as app

This is similar to how Python scripts extend apps like Blender or Sublime Text -- but with Swift and native macOS integration.

### Can I use Agent! without the helper tool?

Yes. The helper tool (root access) is only installed when you explicitly approve it for operations that need root. Most Agent! features work without root:
- File operations in user directories
- App automation via Accessibility
- Web browsing and API calls
- Local LLM inference

You can remove the helper via Settings or by deleting the LaunchDaemon plist.

---

## Privacy Questions

### Does Agent! collect any data?
No. Agent! runs entirely on your Mac. We don't collect analytics, telemetry, or usage data.

### What about API keys?
API keys are stored in your macOS Keychain and never sent anywhere except to the respective API providers (OpenAI, Anthropic, etc.) when you make requests.

### Can Agent! access my passwords?
No. Agent! does not access your passwords, keychain items, or sensitive authentication data. The keychain access is for storing *Agent!'s* API keys, not reading yours.

---

## More Information

- [README.md](../README.md) -- Quick start and overview
- [TECHNICAL.md](TECHNICAL.md) -- Architecture and developer details
- [SECURITY.md](SECURITY.md) -- Security model and entitlements
- [COMPARISON.md](COMPARISON.md) -- Comparisons with similar tools

---

**Agent! -- Not just smart chat. Real action on your Mac.**