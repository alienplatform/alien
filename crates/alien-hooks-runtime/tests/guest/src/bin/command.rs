//! A WASI command for the runtime's command tests: it prints its arguments and what it sees of
//! the host, then does what its first argument asks.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    println!("args: {}", args.join(" "));
    println!("readPasswd: {:?}", std::fs::read("/etc/passwd").map(|b| b.len()));
    println!("vars: {}", std::env::vars().count());
    match args.get(1).map(String::as_str) {
        Some("exit") => std::process::exit(7),
        Some("panic") => panic!("the command was asked to panic"),
        Some("spin") => loop {
            std::hint::black_box(());
        },
        _ => {}
    }
}
