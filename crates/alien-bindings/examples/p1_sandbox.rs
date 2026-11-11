//! Local test driver (not committed): exercises a Sandbox binding from inside a pod.
use alien_bindings::traits::{CommandOutput, CreateSandboxRequest, RunCommandRequest};
use alien_bindings::Bindings;
use futures::StreamExt;
use std::collections::BTreeMap;
use std::time::Duration;

fn req(cmd: &str, args: &[&str]) -> RunCommandRequest {
    serde_json::from_value(serde_json::json!({"command": cmd, "args": args, "timeout": {"secs": 60, "nanos": 0}})).unwrap()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("serve");
    if mode == "serve" {
        let l = tokio::net::TcpListener::bind("0.0.0.0:8081").await.unwrap();
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                use tokio::io::AsyncWriteExt;
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok").await;
            });
        }
    }
    let b = Bindings::from_env().expect("bindings env");
    let sbx = b.sandbox("agents").await.expect("sandbox binding");
    let id = "p1-sbx";
    match mode {
        "get" => {
            let g = sbx.get(&args[2]).await;
            println!("get({}) = {:?}", args[2], g);
        }
        "refused" => {
            let g = sbx.get(id).await;
            println!("{g:?}");
        }
        _ => {
            let mk = || CreateSandboxRequest { sandbox_id: Some(id.to_string()), ..Default::default() };
            let a = sbx.get_or_create(mk()).await;
            println!("get_or_create #1: {:?}", a.as_ref().map(|r| (&r.sandbox.sandbox_id, r.created)));
            let a = a.expect("get_or_create");
            let c = sbx.get_or_create(mk()).await.expect("second get_or_create");
            println!("get_or_create #2: id={} created={}", c.sandbox.sandbox_id, c.created);
            let sid = a.sandbox.sandbox_id.clone();
            println!("get: {:?}", sbx.get(&sid).await.map(|o| o.map(|i| i.state)));
            let mut out = sbx.run_command(&sid, req("/bin/echo", &["hello-from-sandbox"])).await.expect("run");
            while let Some(f) = out.next().await {
                match f {
                    Ok(CommandOutput::Stdout { data, .. }) => print!("stdout: {}", String::from_utf8_lossy(&data)),
                    Ok(CommandOutput::Exit { code, .. }) => println!("exit {code}"),
                    other => println!("frame {other:?}"),
                }
            }
            // web server on 8080 as a job, then preview
            let job = sbx.start_job(&sid, req("python3", &["-m", "http.server", "8080", "--directory", "/tmp"])).await.expect("start_job");
            println!("start_job: {}", job.job_id);
            tokio::time::sleep(Duration::from_secs(3)).await;
            let p = sbx.poll_job(&sid, &job.job_id, None).await.expect("poll_job");
            println!("poll_job: running={} exit={:?} error={:?}", p.running, p.exit, p.error);
            let cap = sbx.preview(&sid, 8080).await;
            match &cap {
                Ok(c) => {
                    println!("preview: endpoint={} ports={:?} expires={}s headers={:?}", c.endpoint, c.allowed_ports, c.expires_in_seconds, c.headers.keys().collect::<Vec<_>>());
                    let client = reqwest::Client::new();
                    let mut r = client.get(&c.endpoint);
                    for (k, v) in &c.headers { r = r.header(k, v); }
                    let resp = r.timeout(Duration::from_secs(10)).send().await;
                    println!("preview GET: {:?}", resp.map(|r| r.status()));
                }
                Err(e) => println!("preview error: {e}"),
            }
            let _ = sbx.cancel_job(&sid, &job.job_id).await;
            sbx.write_files(&sid, BTreeMap::from([("/tmp/p1.txt".to_string(), b"p1".to_vec())])).await.expect("write");
            println!("read_file: {:?}", sbx.read_file(&sid, "/tmp/p1.txt").await.map(|v| String::from_utf8_lossy(&v).to_string()));
            // short job that finishes
            let j2 = sbx.start_job(&sid, req("/bin/echo", &["job-done"])).await.expect("start_job 2");
            for _ in 0..20 {
                let p = sbx.poll_job(&sid, &j2.job_id, None).await.expect("poll 2");
                if !p.running { println!("job2: exit={:?} frames={}", p.exit, p.frames.len()); break; }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if mode == "flow-keep" { println!("kept {sid}"); return; }
            sbx.terminate(&sid).await.expect("terminate");
            println!("terminated; get after = {:?}", sbx.get(&sid).await.map(|o| o.map(|i| i.state)));
        }
    }
}
