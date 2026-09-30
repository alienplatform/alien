use std::io::{self, BufRead};

fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.expect("read compilation request");
        println!("{}", alien_permissions::operations::request_json(&line));
    }
}
