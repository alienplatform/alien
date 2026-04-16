# alien-macros

Procedural macros for the Alien framework:

- **`#[controller]`** — Applied to structs and impl blocks to generate `ResourceController` state machine boilerplate for `alien-infra`. Works with `#[flow_entry]`, `#[handler]`, and `terminal_state!`.
- **`#[alien_event]`** — Wraps async functions with `AlienEvent::in_scope()` for automatic success/failure event tracking.
