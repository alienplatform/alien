//! Alien Agent CLI entry point.
//!
//! All CLI parsing, signal handling, panic-hook plumbing, and the Windows
//! service shim live in [`alien_agent::cli`] so downstream distributions
//! can wrap the same entry point with their own controller-registration
//! hook.

fn main() {
    alien_agent::cli::cli_main();
}
