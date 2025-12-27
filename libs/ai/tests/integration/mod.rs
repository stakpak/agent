//! Integration tests
//!
//! These tests require API keys to be set in environment variables.
//! They are ignored by default and can be run with:
//! `cargo test --test integration -- --ignored`

#[cfg(test)]
mod openai;
