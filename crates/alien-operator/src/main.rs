//! Alien Operator CLI entry point.
//!
//! All CLI parsing, signal handling, panic-hook plumbing, and the Windows
//! service shim live in [`alien_operator::cli`] so downstream distributions
//! can wrap the same entry point with their own controller-registration
//! hook.

fn main() {
    alien_operator::cli::cli_main();
}
