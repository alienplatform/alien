# alien-sdk

Public Rust SDK for Alien applications. Re-exports everything from `alien-bindings` (`pub use alien_bindings::*`) so application code has a single import path.

Owns `AlienContext` (initializing bindings and handling commands) and `WaitUntil`; re-exports the `Bindings` API from `alien-bindings`. Your application code uses `alien_sdk::*` regardless of which cloud platform it runs on.
