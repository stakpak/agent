# Agent! for macOS 26 - Release Notes (1.0.31)

## 🚀 New Features & Improvements

- **Version Bump**: Updated to 1.0.31 with build number 115
- **Improved UI Responsiveness**: Fixed menu spinner by deferring agent runs to next run loop
- **Enhanced Audit Logging**: Added audit log entry for direct agent execution
- **Better File Management**: Updated .gitignore to ignore temporary files and logs
- **Documentation Update**: Clarified Claude reference in README to avoid confusion

## 🐞 Bug Fixes

- **Script Tab Management**: Always close old script tab before re-running to prevent spinner block
- **Xcode Project Configuration**: Fixed project versioning and deployment target settings

## 📦 Infrastructure

- **Added .mailmap**: Standardized author names across commits
- **Created Applications Symlink**: Added symlink for dmg packaging

## 🛠️ Development Enhancements

- **Code Organization**: Added missing import for AgentAudit in TaskUtilities
- **Build System**: Ensured untracked files are added before push

This release focuses on stability, performance, and documentation clarity. The agent is now more responsive and better documented for users and contributors alike.