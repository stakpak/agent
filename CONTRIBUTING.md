# Contributing to Stakpak

Thank you for your interest in contributing to Stakpak! We're excited to have you join our community. This guide will help you get started with contributing to the project.

## 📋 Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Ways to Contribute](#ways-to-contribute)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Code Guidelines](#code-guidelines)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Documentation](#documentation)
- [Getting Help](#getting-help)

## 🤝 Code of Conduct

We are committed to providing a welcoming and inclusive environment for all contributors. Please be respectful, constructive, and collaborative in all interactions.

**Expected Behavior:**
- Use welcoming and inclusive language
- Be respectful of differing viewpoints and experiences
- Gracefully accept constructive criticism
- Focus on what is best for the community
- Show empathy towards other community members

## 🎯 Ways to Contribute

There are many ways to contribute to Stakpak:

### 1. **Report Bugs**
Found a bug? Help us fix it!
- Check if the issue already exists in [GitHub Issues](https://github.com/stakpak/agent/issues)
- If not, create a new issue with a clear title and description
- Include steps to reproduce, expected behavior, and actual behavior
- Add relevant labels (e.g., `bug`, `windows`, `performance`)

### 2. **Suggest Features**
Have an idea for a new feature?
- Open an issue with the `enhancement` label
- Describe the feature, its use case, and potential implementation
- Discuss with maintainers before starting work on large features

### 3. **Fix Issues**
Browse our [open issues](https://github.com/stakpak/agent/issues) and pick one to work on:
- Issues labeled `good first issue` are great for newcomers
- Issues labeled `help wanted` are actively seeking contributors
- Comment on the issue to let others know you're working on it

### 4. **Write Documentation**
Help improve our documentation:
- Fix typos or clarify existing docs
- Add examples and tutorials
- Document undocumented features
- Improve code comments

### 5. **Test on Different Platforms**
We support Linux, macOS, and Windows. Testing on different platforms is valuable:
- See [ISSUES.md](ISSUES.md) for platform testing issues
- Report platform-specific bugs
- Help verify fixes work across platforms

### 6. **Review Pull Requests**
Help review open pull requests:
- Test the changes locally
- Provide constructive feedback
- Check code quality and style

### 7. Content Creation

Help spread the word about Stakpak and make it easier for others to learn:

#### Write Technical Blogs
- Share tutorials, deep-dives, and case studies using Stakpak  
- Publish on your blog, Medium, Dev.to  

#### Create Technical Videos
- Record walkthroughs, demos
- Publish on YouTube, TikTok, or LinkedIn  
- Keep them short, and practical  

#### Host Live Streams
- Stream deploying sessions, feature demos, or Q&A on Twitch, YouTube, or LinkedIn Live  
- Show how you use Stakpak in real-world DevOps workflows  

Content creators will be featured on our social media and newsletter.

## 🚀 Getting Started

### Prerequisites

- **Rust 1.91.0 or later** - Install from [rustup.rs](https://rustup.rs/)
- **Git** - For version control
- **Cargo** - Comes with Rust
- **OpenSSL development libraries** (Linux only):
  - Ubuntu/Debian: `sudo apt-get install pkg-config libssl-dev`
  - Fedora: `sudo dnf install openssl-devel`
  - Arch: `sudo pacman -S openssl`

### Fork and Clone

1. **Fork the repository** on GitHub
2. **Clone your fork:**
   ```bash
   git clone https://github.com/YOUR_USERNAME/agent.git
   cd agent
   ```
3. **Add upstream remote:**
   ```bash
   git remote add upstream https://github.com/stakpak/agent.git
   ```

### Build the Project

```bash
# Build in debug mode (faster compilation)
cargo build

# Build in release mode (optimized)
cargo build --release

# Build a specific crate
cargo build -p stakpak-cli
```

### Run Tests

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p stakpak-shared

# Run tests with output
cargo test --workspace -- --nocapture
```

### Run the CLI Locally

```bash
# Run in development mode
cargo run

# Run with specific flags
cargo run -- --help
cargo run -- --verbose

# Run a specific command
cargo run -- mcp --tool-mode local
```

## 🔄 Development Workflow

### 1. Create a Branch

Always create a new branch for your work:

```bash
# Sync with upstream
git fetch upstream
git checkout main
git merge upstream/main

# Create a feature branch
git checkout -b feature/my-new-feature

# Or for a bug fix
git checkout -b fix/issue-123-description
```

**Branch Naming Conventions:**
- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation changes
- `refactor/` - Code refactoring
- `test/` - Adding or updating tests
- `perf/` - Performance improvements

### 2. Make Changes

- Write clean, readable code
- Follow the [Code Guidelines](#code-guidelines)
- Add tests for new functionality
- Update documentation as needed
- Keep commits focused and atomic

### 3. Test Your Changes

Before submitting, ensure:

```bash
# Code compiles without errors
cargo check --all-targets

# All tests pass
cargo test --workspace

# Code is properly formatted
cargo fmt --check

# No clippy warnings
cargo clippy --all-targets -- -D warnings

# Build succeeds in release mode
cargo build --release
```

### 4. Commit Your Changes

Write clear, descriptive commit messages:

```bash
git add .
git commit -m "feat: add GitHub integration tools

- Implement github_read_issues tool
- Add GitHub API client wrapper
- Include error handling and tests

Closes #123"
```

**Commit Message Format:**
```
<type>: <subject>

<body>

<footer>
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

### 5. Push and Create Pull Request

```bash
# Push your branch
git push origin feature/my-new-feature
```

Then create a Pull Request on GitHub.

## 📝 Code Guidelines

### Rust Style

We follow standard Rust conventions and use `rustfmt` and `clippy`:

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy --all-targets
```

### Key Principles

1. **No Unwrap/Expect**
   - We deny `unwrap()` and `expect()` via clippy
   - Use proper error handling with `Result` and `?` operator
   - Use `match` or `if let` for `Option` types

   ```rust
   // ❌ Bad
   let value = some_option.unwrap();
   
   // ✅ Good
   let value = match some_option {
       Some(v) => v,
       None => return Err("Value not found".into()),
   };
   ```

2. **Error Handling**
   - Use `anyhow::Result` for application errors
   - Use `thiserror` for library errors
   - Provide meaningful error messages

   ```rust
   use anyhow::{Result, Context};
   
   fn read_config() -> Result<Config> {
       let contents = std::fs::read_to_string("config.toml")
           .context("Failed to read config file")?;
       // ...
       Ok(config)
   }
   ```

3. **Async Code**
   - Use `tokio` for async runtime
   - Prefer `async/await` over manual futures
   - Be mindful of blocking operations in async contexts

4. **Code Organization**
   - Keep functions small and focused
   - Use meaningful variable and function names
   - Add comments for complex logic
   - Organize related functionality into modules

5. **Security**
   - Never log or print secrets
   - Use the secret redaction system for sensitive data
   - Validate all external input
   - Be cautious with file system operations

### Project Structure

```
cli/                    # CLI binary crate
├── src/
│   ├── commands/      # CLI commands
│   ├── config.rs      # Configuration handling
│   └── main.rs
tui/                    # TUI crate
├── src/
│   └── services/      # TUI services
libs/
├── api/               # API client library
├── mcp/
│   ├── client/        # MCP client
│   └── server/        # MCP server and tools
└── shared/            # Shared utilities
    ├── src/
    │   ├── secrets/   # Secret detection
    │   └── models/    # Data models
```

## 🧪 Testing

### Writing Tests

Add tests for all new functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_works() {
        let result = my_function();
        assert_eq!(result, expected_value);
    }

    #[tokio::test]
    async fn test_async_feature() {
        let result = my_async_function().await;
        assert!(result.is_ok());
    }
}
```

### Test Categories

1. **Unit Tests** - Test individual functions/modules
2. **Integration Tests** - Test component interactions
3. **Platform Tests** - Test on Linux, macOS, Windows

### Running Specific Tests

```bash
# Run tests matching a pattern
cargo test test_name

# Run tests in a specific file
cargo test --test integration_test

# Run doc tests
cargo test --doc

# Run with verbose output
cargo test -- --test-threads=1 --nocapture
```

## 📤 Submitting Changes

### Pull Request Process

1. **Update Your Branch**
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Create Pull Request**
   - Use a clear, descriptive title
   - Reference related issues (e.g., "Fixes #123")
   - Describe what changes you made and why
   - Include screenshots for UI changes
   - Add any breaking changes to the description

3. **PR Description Template**
   ```markdown
   ## Description
   Brief description of what this PR does.

   ## Related Issues
   Fixes #123

   ## Changes Made
   - Change 1
   - Change 2
   - Change 3

   ## Testing
   - [ ] All tests pass locally
   - [ ] Added tests for new functionality
   - [ ] Tested on Linux/macOS/Windows (specify which)

   ## Screenshots (if applicable)
   
   ## Breaking Changes
   None / List any breaking changes
   ```

4. **Review Process**
   - Maintainers will review your PR
   - Address feedback and push updates
   - Once approved, a maintainer will merge your PR

### PR Checklist

Before submitting, ensure:

- [ ] Code follows style guidelines
- [ ] All tests pass (`cargo test --workspace`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] Documentation is updated
- [ ] Commit messages are clear
- [ ] Branch is up to date with `main`
- [ ] No merge conflicts

## 📚 Documentation

### Code Documentation

- Add doc comments for public APIs:
  ```rust
  /// Reads a configuration file from the specified path.
  ///
  /// # Arguments
  /// * `path` - Path to the configuration file
  ///
  /// # Returns
  /// Returns `Ok(Config)` on success, or an error if the file cannot be read
  ///
  /// # Example
  /// ```
  /// let config = read_config("~/.stakpak/config.toml")?;
  /// ```
  pub fn read_config(path: &str) -> Result<Config> {
      // ...
  }
  ```

### User Documentation

- Update README.md for user-facing changes
- Add examples in `examples/` directory
- Update the docs site (if applicable)

### Changelog

- Add a brief note about your changes
- Follow the format of existing entries
- Categorize as Added/Changed/Fixed/Removed

## 🆘 Getting Help

### Resources

- **Documentation:** [stakpak.gitbook.io](https://stakpak.gitbook.io/docs)
- **Issues:** [GitHub Issues](https://github.com/stakpak/agent/issues)
- **Discussions:** [GitHub Discussions](https://github.com/stakpak/agent/discussions)
- **Website:** [stakpak.dev](https://stakpak.dev)

### Questions

- Check existing issues and discussions first
- Create a new discussion for general questions
- Use issues only for bugs and feature requests
- Tag issues appropriately for better visibility

### Stuck?

If you're stuck or need help:
1. Check the existing documentation
2. Search closed issues and PRs
3. Ask in GitHub Discussions
4. Reach out to maintainers

## 🎉 Recognition

We value all contributions! Contributors will be:
- Listed in our contributors list
- Mentioned in release notes for significant contributions
- Thanked in commit messages and PR descriptions

## 📜 License

By contributing to Stakpak, you agree that your contributions will be licensed under the same license as the project.

---

**Thank you for contributing to Stakpak! 🚀**

Your contributions help make DevOps more secure and efficient for everyone.
