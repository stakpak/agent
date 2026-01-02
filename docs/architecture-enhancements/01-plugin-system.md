# Enhancement Proposal: Plugin System Architecture

## Overview

OpenCode implements a plugin system that allows third-party extensions to add authentication methods, providers, and custom functionality. Stakpak currently has a monolithic architecture where all providers and features are compiled into the binary.

## Current Stakpak Architecture

```
cli/
├── src/
│   ├── commands/          # All commands hardcoded
│   ├── onboarding/        # Provider setup hardcoded
│   └── config.rs          # Static provider configuration
libs/
├── ai/
│   └── providers/         # Providers compiled in
│       ├── anthropic/
│       ├── gemini/
│       └── openai/
```

## OpenCode Plugin Architecture

```typescript
// packages/opencode/src/plugin/index.ts
export namespace Plugin {
  const state = Instance.state(async () => {
    const plugins = []
    // Dynamic plugin loading from npm packages
    plugins.push("opencode-anthropic-auth@0.0.5")
    // ...
  })
}
```

OpenCode plugins can:
- Add authentication methods (OAuth, API keys)
- Register new providers
- Add custom commands
- Hook into the request/response lifecycle

## Proposed Enhancement

### Phase 1: Plugin Trait System

Create a plugin trait that defines the interface for extensions:

```rust
// libs/plugin/src/lib.rs
pub trait StakpakPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    
    // Optional hooks
    fn on_init(&self, _ctx: &PluginContext) -> Result<()> { Ok(()) }
    fn auth_methods(&self) -> Vec<AuthMethod> { vec![] }
    fn providers(&self) -> Vec<Box<dyn Provider>> { vec![] }
    fn commands(&self) -> Vec<Command> { vec![] }
    fn on_request(&self, _req: &mut Request) -> Result<()> { Ok(()) }
    fn on_response(&self, _res: &mut Response) -> Result<()> { Ok(()) }
}
```

### Phase 2: Dynamic Loading (Optional)

For Rust, dynamic loading is more complex than TypeScript. Options:

1. **WASM Plugins**: Load plugins as WebAssembly modules
2. **Shared Libraries**: Use `libloading` for `.so`/`.dylib` plugins
3. **Scripting**: Embed Lua/Rhai for lightweight plugins

Recommended: Start with compiled-in plugins using the trait system, then add WASM support later.

### Phase 3: Plugin Registry

```rust
// libs/plugin/src/registry.rs
pub struct PluginRegistry {
    plugins: Vec<Box<dyn StakpakPlugin>>,
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: Box<dyn StakpakPlugin>) {
        self.plugins.push(plugin);
    }
    
    pub fn get_auth_methods(&self) -> Vec<AuthMethod> {
        self.plugins.iter()
            .flat_map(|p| p.auth_methods())
            .collect()
    }
}
```

## Benefits

1. **Extensibility**: Users can add custom providers without forking
2. **Separation of Concerns**: Core logic separate from provider implementations
3. **Easier Testing**: Mock plugins for testing
4. **Community Contributions**: Lower barrier for adding new providers

## Implementation Effort

| Phase | Effort | Priority |
|-------|--------|----------|
| Plugin Trait System | 2-3 days | High |
| Plugin Registry | 1-2 days | High |
| WASM Support | 1-2 weeks | Low |
| Plugin CLI Commands | 2-3 days | Medium |

## Migration Path

1. Define `StakpakPlugin` trait
2. Refactor existing providers to implement the trait
3. Create `PluginRegistry` to manage plugins
4. Add plugin configuration to `config.toml`
5. (Future) Add dynamic loading support

## Files to Create/Modify

```
libs/
├── plugin/                    # NEW
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── registry.rs
│       ├── traits.rs
│       └── context.rs
├── ai/
│   └── providers/
│       ├── mod.rs            # MODIFY: implement Plugin trait
│       └── ...
cli/
└── src/
    └── main.rs               # MODIFY: initialize plugin registry
```

## Example Plugin Implementation

```rust
// Example: Custom provider plugin
pub struct MyCustomProvider;

impl StakpakPlugin for MyCustomProvider {
    fn name(&self) -> &str { "my-custom-provider" }
    fn version(&self) -> &str { "0.1.0" }
    
    fn auth_methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::ApiKey {
            name: "MY_PROVIDER_API_KEY".to_string(),
            env_var: "MY_PROVIDER_API_KEY".to_string(),
        }]
    }
    
    fn providers(&self) -> Vec<Box<dyn Provider>> {
        vec![Box::new(MyProviderImpl::new())]
    }
}
```
