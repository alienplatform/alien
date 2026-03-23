//! Colored terminal output helpers (no TUI).

/// Print a success message
pub fn success(msg: &str) {
    eprintln!("\x1b[32m✓\x1b[0m {}", msg);
}

/// Print an info message
pub fn info(msg: &str) {
    eprintln!("\x1b[34mℹ\x1b[0m {}", msg);
}

/// Print a warning message
pub fn warn(msg: &str) {
    eprintln!("\x1b[33m⚠\x1b[0m {}", msg);
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("\x1b[31m✗\x1b[0m {}", msg);
}

/// Print a step in a process
pub fn step(num: usize, total: usize, msg: &str) {
    eprintln!("\x1b[36m[{}/{}]\x1b[0m {}", num, total, msg);
}

/// Print a status line
pub fn status(label: &str, value: &str) {
    eprintln!("  \x1b[1m{:<20}\x1b[0m {}", label, value);
}

/// Print a header
pub fn header(msg: &str) {
    eprintln!("\n\x1b[1;4m{}\x1b[0m\n", msg);
}
