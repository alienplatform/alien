//! The guest side of the hooks ABI, written out by hand, around test hooks. Outputs are plain
//! text so the module needs no dependencies.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Once;

const MANIFEST: &str = r#"{"extension":"test-guest","hooks":[
{"name":"echo","kind":"cloudformationModifier"},
{"name":"fail","kind":"cloudformationModifier"},
{"name":"spin","kind":"cloudformationModifier"},
{"name":"allocate-forever","kind":"cloudformationModifier"},
{"name":"output-of-size","kind":"cloudformationModifier"},
{"name":"panic","kind":"cloudformationModifier"},
{"name":"recurse","kind":"cloudformationModifier"},
{"name":"exit","kind":"cloudformationModifier"},
{"name":"open-file","kind":"cloudformationModifier"},
{"name":"read-env","kind":"cloudformationModifier"},
{"name":"socket","kind":"cloudformationModifier"},
{"name":"counter","kind":"cloudformationModifier"},
{"name":"print","kind":"cloudformationModifier"},
{"name":"hashmap-order","kind":"cloudformationModifier"},
{"name":"random","kind":"cloudformationModifier"},
{"name":"clock","kind":"cloudformationModifier"},
{"name":"fail-coded","kind":"cloudformationModifier"},
{"name":"rds-iam","kind":"awsInterceptor","matches":{"service":"rds","operations":["CreateDBCluster"]}}
]}"#;

fn run(name: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    match name {
        "echo" => Ok(input.to_vec()),
        "fail" => Err(format!("refused {} bytes", input.len())),
        "fail-coded" => {
            Err(r#"{"code":"ZONE_LIMIT","message":"too many zones","retryable":true}"#.to_string())
        }
        "fail-text-code" => Err(
            r#"{"code":"x\nTransform: AWS::Serverless","message":"m","retryable":false}"#
                .to_string(),
        ),
        "spin" => {
            let mut n: u64 = 0;
            loop {
                n = std::hint::black_box(n.wrapping_add(1));
            }
        }
        "allocate-forever" => {
            let mut kept: Vec<Vec<u8>> = Vec::new();
            loop {
                kept.push(vec![1u8; 1 << 20]);
                std::hint::black_box(&kept);
            }
        }
        "output-of-size" => {
            let size: usize = std::str::from_utf8(input)
                .map_err(|e| e.to_string())?
                .parse()
                .map_err(|e: std::num::ParseIntError| e.to_string())?;
            Ok(vec![b'x'; size])
        }
        "panic" => panic!("the hook panicked on purpose"),
        "recurse" => {
            #[allow(unconditional_recursion)]
            fn deep(n: u64) -> u64 {
                let pad = std::hint::black_box([n; 64]);
                deep(n + 1) + pad[3]
            }
            Ok(deep(0).to_le_bytes().to_vec())
        }
        "exit" => std::process::exit(3),
        "open-file" => Ok(lines(&[
            (
                "readPasswd",
                format!("{:?}", std::fs::read("/etc/passwd").map(|b| b.len())),
            ),
            (
                "readDirRoot",
                format!("{:?}", std::fs::read_dir("/").map(|d| d.count())),
            ),
            (
                "readDirDot",
                format!("{:?}", std::fs::read_dir(".").map(|d| d.count())),
            ),
            (
                "writeTmp",
                format!("{:?}", std::fs::write("/tmp/hooks-test-guest", b"x")),
            ),
        ])),
        "read-env" => Ok(lines(&[
            ("home", format!("{:?}", std::env::var("HOME"))),
            (
                "secret",
                format!("{:?}", std::env::var("HOOKS_RUNTIME_TEST_SECRET")),
            ),
            ("vars", format!("{}", std::env::vars().count())),
            ("args", format!("{}", std::env::args().count())),
        ])),
        "socket" => Ok(lines(&[
            (
                "tcp",
                format!(
                    "{:?}",
                    std::net::TcpStream::connect("127.0.0.1:1").map(|_| ())
                ),
            ),
            (
                "udp",
                format!("{:?}", std::net::UdpSocket::bind("0.0.0.0:0").map(|_| ())),
            ),
            (
                "dns",
                format!(
                    "{:?}",
                    std::net::ToSocketAddrs::to_socket_addrs("example.com:443").map(|a| a.count())
                ),
            ),
        ])),
        "counter" => {
            static CALLS: AtomicU64 = AtomicU64::new(0);
            Ok((CALLS.fetch_add(1, Ordering::Relaxed) + 1)
                .to_string()
                .into_bytes())
        }
        "print" => {
            println!("a line on stdout");
            eprintln!("a line on stderr");
            Ok(input.to_vec())
        }
        "hashmap-order" => {
            let map: HashMap<String, u32> = (0..64).map(|i| (format!("key-{i}"), i)).collect();
            Ok(map
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
                .into_bytes())
        }
        "random" => {
            let mut bytes = [0u8; 32];
            // SAFETY: the buffer is valid for its length.
            let errno = unsafe { random_get(bytes.as_mut_ptr(), bytes.len()) };
            if errno != 0 {
                return Err(format!("random_get returned {errno}"));
            }
            Ok(bytes.to_vec())
        }
        "clock" => {
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos());
            let started = std::time::Instant::now();
            Ok(lines(&[
                ("wallNanos", format!("{wall:?}")),
                (
                    "monotonicElapsedNanos",
                    started.elapsed().as_nanos().to_string(),
                ),
            ]))
        }
        "rds-iam" => Ok(input.to_vec()),
        other => Err(format!("no hook named {other:?}")),
    }
}

fn lines(fields: &[(&str, String)]) -> Vec<u8> {
    fields
        .iter()
        .map(|(key, value)| format!("{key}: {value}\n"))
        .collect::<String>()
        .into_bytes()
}

#[link(wasm_import_module = "wasi_snapshot_preview1")]
extern "C" {
    fn random_get(buf: *mut u8, len: usize) -> u16;
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

/// SAFETY: `ptr` and `len` come from `alien_alloc`, which leaks a `Vec<u8>` of that capacity.
unsafe fn take(ptr: i32, len: i32) -> Vec<u8> {
    let mut bytes = Vec::from_raw_parts(ptr as *mut u8, 0, len as usize);
    bytes.set_len(len as usize);
    bytes
}

#[no_mangle]
pub extern "C" fn alien_hooks_abi_v1() {}

#[no_mangle]
pub extern "C" fn alien_alloc(len: i32) -> i32 {
    Vec::<u8>::with_capacity(len as usize).leak().as_mut_ptr() as i32
}

#[no_mangle]
pub extern "C" fn alien_hooks_manifest() -> i64 {
    pack(Ok(MANIFEST.as_bytes().to_vec()))
}

#[no_mangle]
pub extern "C" fn alien_hooks_call(name_ptr: i32, name_len: i32, in_ptr: i32, in_len: i32) -> i64 {
    static PANIC_HOOK: Once = Once::new();
    PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| eprintln!("the hook panicked: {info}")));
    });
    // SAFETY: the host wrote both buffers into memory it got from `alien_alloc`.
    let (name, input) = unsafe { (take(name_ptr, name_len), take(in_ptr, in_len)) };
    pack(run(&String::from_utf8_lossy(&name), &input))
}
