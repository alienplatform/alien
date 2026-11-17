//! The guest side of the controller ABI, written out by hand, around test steps. A step does
//! what its resource's `probe` names, with the resource's `arg`, and reports what happened as
//! the resource's progress. The module has no dependencies, so it reads the few fields it needs
//! from the step's JSON itself.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};

#[link(wasm_import_module = "alien_cloud_v1")]
extern "C" {
    #[link_name = "send"]
    fn cloud_send(ptr: *const u8, len: usize) -> u64;
}

#[link(wasm_import_module = "alien_http_v1")]
extern "C" {
    #[link_name = "send"]
    fn http_send(ptr: *const u8, len: usize) -> u64;
}

#[link(wasm_import_module = "wasi_snapshot_preview1")]
extern "C" {
    fn random_get(buf: *mut u8, len: usize) -> u16;
}

type Send = unsafe extern "C" fn(*const u8, usize) -> u64;

/// Sends `request` through an import and returns the host's reply, or its refusal.
fn send(import: Send, request: &str) -> Result<String, String> {
    // SAFETY: the host reads the request from this module's memory and returns where it wrote
    // the reply, in memory it got from `alien_alloc`.
    let reply = unsafe {
        let packed = import(request.as_ptr(), request.len());
        let len = ((packed >> 32) & 0x7fff_ffff) as usize;
        let bytes = Vec::from_raw_parts((packed & 0xffff_ffff) as usize as *mut u8, len, len);
        (
            String::from_utf8_lossy(&bytes).into_owned(),
            packed >> 63 == 1,
        )
    };
    match reply {
        (reply, false) => Ok(reply),
        (refusal, true) => Err(refusal),
    }
}

fn outcome(result: Result<String, String>) -> String {
    match result {
        Ok(reply) => format!("OK {reply}"),
        Err(refusal) => format!("ERR {refusal}"),
    }
}

/// What a step answers with.
enum Answer {
    /// The step ran, with this progress.
    Stepped(String),
    /// The step failed with this code, and may or may not be tried again.
    Failed { code: String, retryable: bool },
    /// These bytes, whatever they are.
    Raw(Vec<u8>),
    /// The module can't answer, and says why.
    NoReply(String),
}

static STEPS: AtomicU64 = AtomicU64::new(0);
static LEFT_BEHIND: Mutex<String> = Mutex::new(String::new());

fn run(probe: &str, arg: &str, input: &str) -> Answer {
    Answer::Stepped(match probe {
        "noop" | "never-deleted" => probe.to_string(),
        "echo" => input.to_string(),
        "cloud" => outcome(send(cloud_send, arg)),
        "http" => outcome(send(http_send, arg)),
        "cloud-many" => {
            let count: u64 = field(arg, "count")
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            let request = field(arg, "request").unwrap_or("{}");
            let refused = (0..count).find_map(|sent| {
                send(cloud_send, request)
                    .err()
                    .map(|refusal| format!("ERR after {sent}: {refusal}"))
            });
            refused.unwrap_or_else(|| format!("sent {count}"))
        }
        // Tells the cloud what step this is, then does what the cloud's answer says: the
        // module stops after a call the cloud already acted on.
        "report-then" => {
            let report = format!(
                r#"{{"cloud":"aws","service":"ec2","method":"POST","path":"/report","body":"{}"}}"#,
                escape(input)
            );
            let reply = send(cloud_send, &report).unwrap_or_else(|refusal| refusal);
            if reply.contains("trap") {
                panic!("the cloud said to trap");
            }
            "reported".to_string()
        }
        "loop" => {
            let mut n: u64 = 0;
            loop {
                n = std::hint::black_box(n.wrapping_add(1));
            }
        }
        "alloc" => {
            let mut kept: Vec<Vec<u8>> = Vec::new();
            loop {
                kept.push(vec![1u8; 1 << 20]);
                std::hint::black_box(&kept);
            }
        }
        "panic" => panic!("the step panicked on purpose"),
        // What an earlier step of this module left in memory, then leaves `arg` there.
        "left-behind" => {
            let steps = STEPS.fetch_add(1, Ordering::SeqCst) + 1;
            let mut left = LEFT_BEHIND.lock().unwrap();
            let found = std::mem::replace(&mut *left, arg.to_string());
            format!("steps={steps} found={found:?}")
        }
        "sandbox" => format!(
            "env={} args={} readDir={:?} readFile={:?} connect={:?}",
            std::env::vars().count(),
            std::env::args().count(),
            std::fs::read_dir("/").map(|entries| entries.count()),
            std::fs::read("/etc/passwd").map(|bytes| bytes.len()),
            std::net::TcpStream::connect("169.254.169.254:80").map(|_| "connected"),
        ),
        "random" => {
            let mut bytes = [0u8; 16];
            // SAFETY: the host fills the 16 bytes at the pointer.
            unsafe { random_get(bytes.as_mut_ptr(), bytes.len()) };
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        }
        // Sends `request`, then fails whatever the answer, with `retryable` as `arg` says.
        "cloud-then-fail" => {
            let _ = send(cloud_send, field(arg, "request").unwrap_or("{}"));
            return Answer::Failed {
                code: "PROBE_FAILED".to_string(),
                retryable: field(arg, "retryable") == Some("true"),
            };
        }
        // Sends `request`, then does what the probe `then` names.
        "cloud-then" => {
            let _ = send(cloud_send, field(arg, "request").unwrap_or("{}"));
            return run(
                unquoted(field(arg, "then").unwrap_or("\"noop\"")),
                "null",
                input,
            );
        }
        "fail" => {
            return Answer::Failed {
                code: unquoted(field(arg, "code").unwrap_or("\"PROBE_FAILED\"")).to_string(),
                retryable: field(arg, "retryable") == Some("true"),
            }
        }
        "garbage" => return Answer::Raw(b"not a step reply".to_vec()),
        "big-reply" => return Answer::Raw(vec![b'x'; arg.parse().unwrap_or(0)]),
        "no-reply" => return Answer::NoReply(unquoted(arg).repeat(64)),
        other => format!("unknown probe {other}"),
    })
}

fn step(input: &str) -> Result<Vec<u8>, String> {
    let step_id = field(input, "stepId").ok_or("the step has no stepId")?;
    let resource = field(input, "resource").ok_or("the step has no resource")?;
    let probe = unquoted(field(resource, "probe").unwrap_or("\"noop\""));
    let arg = field(resource, "arg").unwrap_or("null");
    let deleting = ["delete-start", "teardown-start"]
        .iter()
        .any(|transition| field(input, "transitions").is_some_and(|all| all.contains(transition)))
        || field(input, "state").is_some_and(|state| state.contains("\"deleting\":true"));

    let (status, state) = match (deleting, probe) {
        (true, "never-deleted") => ("deleting", r#"{"deleting":true}"#.to_string()),
        (true, _) => ("deleted", r#"{"deleted":true}"#.to_string()),
        (false, _) => ("running", format!(r#"{{"probe":"{}"}}"#, escape(probe))),
    };
    let view = |status: &str, progress: &str| {
        format!(
            r#"{{"state":{state},"status":"{status}","progress":"{}","outputs":null,"bindingParams":null,"onFailureStatus":"provision-failed","updateStart":{{"outcome":"allowed","status":"updating"}},"deleteStart":{{"outcome":"allowed","status":"deleting"}},"teardownStart":{{"outcome":"allowed","status":"deleting"}}}}"#,
            escape(progress)
        )
    };
    let reply = match run(probe, arg, input) {
        Answer::Stepped(progress) => format!(
            r#"{{"protocol":1,"stepId":{step_id},"result":"stepped","controller":{},"suggestedDelayMs":null}}"#,
            view(status, &progress)
        ),
        Answer::Failed { code, retryable } => format!(
            r#"{{"protocol":1,"stepId":{step_id},"result":"failed","controller":{},"error":{{"code":"{}","message":"the probe failed","retryable":{retryable}}}}}"#,
            view("provisioning", ""),
            escape(&code)
        ),
        Answer::Raw(bytes) => return Ok(bytes),
        Answer::NoReply(why) => return Err(why),
    };
    Ok(reply.into_bytes())
}

/// The raw text of `key`'s value in the JSON object `object`.
fn field<'a>(object: &'a str, key: &str) -> Option<&'a str> {
    let bytes = object.as_bytes();
    let mut at = skip_space(bytes, 0);
    if bytes.get(at) != Some(&b'{') {
        return None;
    }
    at += 1;
    loop {
        at = skip_space(bytes, at);
        if bytes.get(at) != Some(&b'"') {
            return None;
        }
        let name_end = value_end(bytes, at);
        let name = &object[at + 1..name_end - 1];
        at = skip_space(bytes, name_end);
        if bytes.get(at) != Some(&b':') {
            return None;
        }
        at = skip_space(bytes, at + 1);
        let end = value_end(bytes, at);
        if name == key {
            return Some(&object[at..end]);
        }
        at = skip_space(bytes, end);
        if bytes.get(at) != Some(&b',') {
            return None;
        }
        at += 1;
    }
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|byte| byte.is_ascii_whitespace()) {
        at += 1;
    }
    at
}

/// Where the JSON value that starts at `at` ends.
fn value_end(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0usize;
    let mut in_string = false;
    while let Some(&byte) = bytes.get(at) {
        at += 1;
        if in_string {
            match byte {
                b'\\' => at += 1,
                b'"' => {
                    in_string = false;
                    if depth == 0 {
                        return at;
                    }
                }
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return at;
                }
            }
            b',' | b'}' | b']' if depth == 0 => return at - 1,
            _ => {}
        }
    }
    at.min(bytes.len())
}

/// A JSON string without its quotes. The tests' strings need no unescaping.
fn unquoted(raw: &str) -> &str {
    raw.trim_matches('"')
}

/// `text` as the inside of a JSON string.
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}

fn pack(result: Result<Vec<u8>, String>) -> i64 {
    let (output, error) = match result {
        Ok(bytes) => (bytes, false),
        Err(message) => (message.into_bytes(), true),
    };
    let output = output.leak();
    let ptr = output.as_ptr() as u32 as u64;
    let len = output.len() as u64;
    (ptr | (len << 32) | if error { 1 << 63 } else { 0 }) as i64
}

#[no_mangle]
pub extern "C" fn alien_controller_abi_v1() {}

#[no_mangle]
pub extern "C" fn alien_alloc(len: i32) -> i32 {
    Vec::<u8>::with_capacity(len as usize).leak().as_mut_ptr() as i32
}

#[no_mangle]
pub extern "C" fn alien_controller_step(ptr: i32, len: i32) -> i64 {
    static PANIC_HOOK: Once = Once::new();
    PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| eprintln!("the step panicked: {info}")));
    });
    // SAFETY: the host wrote the step into memory it got from `alien_alloc`, which leaks a
    // `Vec<u8>` of that capacity.
    let input = unsafe {
        let mut bytes = Vec::from_raw_parts(ptr as *mut u8, 0, len as usize);
        bytes.set_len(len as usize);
        bytes
    };
    pack(step(&String::from_utf8_lossy(&input)))
}
