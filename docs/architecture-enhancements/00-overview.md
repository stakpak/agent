# Stakpak CLI Architecture Enhancement Proposals

## Executive Summary

This document series analyzes architectural patterns from OpenCode and proposes enhancements to the Stakpak CLI. The focus is on **architectural separation and modularity**, not feature parity.

## Current Stakpak Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         CLI (cli/)                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   main.rs   │  │  commands/  │  │      onboarding/        │  │
│  │  (monolith) │  │   agent/    │  │  (provider setup)       │  │
│  └─────────────┘  │   mcp/      │  └─────────────────────────┘  │
│                   │   acp/      │                                │
│                   └─────────────┘                                │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         TUI (tui/)                               │
│  ┌─────────────┐  ┌─────────────────────────────────────────┐   │
│  │   app.rs    │  │              services/                   │   │
│  │  (god obj)  │  │  handlers/ approval/ sessions/ etc.     │   │
│  └─────────────┘  └─────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        LIBS (libs/)                              │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐            │
│  │   ai/   │  │  api/   │  │  mcp/   │  │ shared/ │            │
│  │providers│  │local/   │  │client/  │  │ models/ │            │
│  │         │  │remote/  │  │server/  │  │ secrets │            │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘            │
└─────────────────────────────────────────────────────────────────┘
```

### Current Strengths ✓

1. **Rust Foundation**: Performance, memory safety, strong typing
2. **Workspace Structure**: Good crate separation (cli, tui, libs)
3. **MCP Implementation**: Solid client/server/proxy architecture
4. **Secret Management**: Comprehensive redaction system
5. **Provider Abstraction**: `AgentProvider` trait for local/remote

### Current Weaknesses ✗

1. **Tight Coupling**: Components directly reference each other
2. **Monolithic App**: TUI `App` struct holds too much state
3. **No Event System**: Direct function calls for communication
4. **API Key Only**: No OAuth/subscription-based auth
5. **No HTTP API**: Can't integrate with IDEs or web UIs
6. **Hardcoded Tools**: Tools embedded in MCP server
7. **No Plugin System**: Can't extend without forking

## OpenCode Architecture (Reference)

```
┌─────────────────────────────────────────────────────────────────┐
│                      CLI Layer                                   │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   index.ts  │  │  cmd/*.ts   │  │      tui/               │  │
│  │  (yargs)    │  │  (commands) │  │  (blessed terminal)     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    HTTP Server (Hono.js)                         │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  /session  /provider  /mcp  /tool  /config  (REST API)  │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│    Providers    │  │   Event Bus     │  │    Plugins      │
│  (ai-sdk based) │  │  (pub/sub)      │  │  (npm packages) │
└─────────────────┘  └─────────────────┘  └─────────────────┘
          │                   │                   │
          └───────────────────┼───────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Core Services                                 │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐            │
│  │ Session │  │  Auth   │  │  Tool   │  │ Project │            │
│  │ Manager │  │ Manager │  │Registry │  │Instance │            │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘            │
└─────────────────────────────────────────────────────────────────┘
```

## Enhancement Proposals

| # | Proposal | Priority | Effort | Impact |
|---|----------|----------|--------|--------|
| 01 | [Plugin System](./01-plugin-system.md) | Medium | 1-2 weeks | High |
| 02 | [Event Bus System](./02-event-bus-system.md) | High | 3-5 days | High |
| 03 | [HTTP Server API](./03-http-server-api.md) | Medium | 1-2 weeks | High |
| 04 | [OAuth Provider Auth](./04-oauth-provider-auth.md) | High | 1 week | High |
| 05 | [Slash Commands](./05-slash-commands.md) | Low | 3-5 days | Medium |
| 06 | [Project Instance Context](./06-project-instance-context.md) | High | 1 week | High |
| 07 | [Modular Tool System](./07-modular-tool-system.md) | High | 1 week | High |

## Recommended Implementation Order

### Phase 1: Foundation (Weeks 1-2)
1. **Event Bus System** - Enables decoupled communication
2. **Project Instance Context** - Clean state management

### Phase 2: Core Improvements (Weeks 3-4)
3. **Modular Tool System** - Better tool organization
4. **OAuth Provider Auth** - User convenience (Claude Max/Pro)

### Phase 3: Extensibility (Weeks 5-6)
5. **HTTP Server API** - IDE integration, web UI
6. **Plugin System** - Third-party extensions

### Phase 4: Polish (Week 7)
7. **Slash Commands** - UX improvement

## Architecture Comparison

| Aspect | Stakpak (Current) | OpenCode | Proposed Stakpak |
|--------|-------------------|----------|------------------|
| Language | Rust | TypeScript/Bun | Rust |
| Communication | Direct calls | Event Bus | Event Bus |
| State | Scattered | Instance pattern | Instance pattern |
| Tools | MCP-embedded | Modular registry | Modular registry |
| Auth | API keys only | OAuth + API keys | OAuth + API keys |
| API | None | HTTP REST + SSE | HTTP REST + SSE |
| Plugins | None | npm packages | Trait-based |
| Commands | Keyboard only | Slash commands | Slash commands |

## Key Architectural Principles

### 1. Separation of Concerns
- UI layer should only handle rendering
- Business logic in service layer
- Data access in repository layer

### 2. Dependency Inversion
- Depend on abstractions (traits), not concretions
- Use dependency injection for testability

### 3. Event-Driven Architecture
- Components communicate via events
- Loose coupling, high cohesion

### 4. Plugin Architecture
- Core functionality via traits
- Extensions implement traits
- Registry manages plugins

## Files Overview

```
docs/architecture-enhancements/
├── 00-overview.md              # This file
├── 01-plugin-system.md         # Plugin trait system
├── 02-event-bus-system.md      # Pub/sub event bus
├── 03-http-server-api.md       # REST API with Axum
├── 04-oauth-provider-auth.md   # OAuth for Claude Max/Pro
├── 05-slash-commands.md        # /command system
├── 06-project-instance-context.md  # State management
└── 07-modular-tool-system.md   # Tool registry pattern
```

## Next Steps

1. Review proposals with team
2. Prioritize based on user needs
3. Create implementation tickets
4. Start with Phase 1 foundation work
5. Iterate based on feedback

## References

- [OpenCode Repository](https://github.com/sst/opencode)
- [Stakpak CLI Repository](https://github.com/stakpak/cli)
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/)
- [Event-Driven Architecture](https://martinfowler.com/articles/201701-event-driven.html)
