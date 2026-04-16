# alien-sdk

Public Rust SDK for Alien applications. Re-exports everything from `alien-bindings` (`pub use alien_bindings::*`) so application code has a single import path.

Provides `AlienContext` for initializing bindings and handling commands. Your application code uses `alien_sdk::*` regardless of which cloud platform it runs on.
