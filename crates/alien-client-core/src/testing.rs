//! Test support for the interceptor hook: a native interceptor built from closures, and a local
//! HTTP server that records the bytes each request put on the wire.

use crate::error::Result;
use crate::interceptor::{CallInterceptor, Cloud, HookRequest, HookResponse};

type MatchFn = dyn Fn(&HookRequest) -> bool + Send + Sync;
type RequestFn = dyn Fn(&mut HookRequest) -> Result<()> + Send + Sync;
type ResponseFn = dyn Fn(&HookRequest, &mut HookResponse) -> Result<()> + Send + Sync;

/// An interceptor for calls to one `(cloud, service)` whose hooks are closures. It matches every
/// such call and both hooks leave the call unchanged until set.
pub struct TestInterceptor {
    extension: String,
    cloud: Cloud,
    service: String,
    when: Box<MatchFn>,
    request: Box<RequestFn>,
    response: Box<ResponseFn>,
}

impl TestInterceptor {
    pub fn new(extension: &str, cloud: Cloud, service: &str) -> Self {
        Self {
            extension: extension.to_string(),
            cloud,
            service: service.to_string(),
            when: Box::new(|_| true),
            request: Box::new(|_| Ok(())),
            response: Box::new(|_, _| Ok(())),
        }
    }

    /// Narrows the calls it matches within its `(cloud, service)`.
    pub fn when(mut self, matches: impl Fn(&HookRequest) -> bool + Send + Sync + 'static) -> Self {
        self.when = Box::new(matches);
        self
    }

    pub fn on_request(
        mut self,
        hook: impl Fn(&mut HookRequest) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.request = Box::new(hook);
        self
    }

    pub fn on_response(
        mut self,
        hook: impl Fn(&HookRequest, &mut HookResponse) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.response = Box::new(hook);
        self
    }
}

impl CallInterceptor for TestInterceptor {
    fn extension(&self) -> &str {
        &self.extension
    }

    fn matches(&self, request: &HookRequest) -> Result<bool> {
        Ok(request.cloud == self.cloud && request.service == self.service && (self.when)(request))
    }

    fn modify_request(&self, request: &mut HookRequest) -> Result<()> {
        (self.request)(request)
    }

    fn modify_response(&self, request: &HookRequest, response: &mut HookResponse) -> Result<()> {
        (self.response)(request, response)
    }
}

pub mod capture {
    //! A local HTTP/1.1 server that records each request and answers it from a function.

    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Debug, Clone)]
    pub struct Captured {
        pub method: String,
        /// The request target: path and query.
        pub target: String,
        /// Names lowercased, in arrival order.
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Captured {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }

        pub fn body_text(&self) -> String {
            String::from_utf8_lossy(&self.body).to_string()
        }
    }

    pub struct Reply {
        pub status: u16,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
    }

    impl Reply {
        pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
            Self {
                status,
                headers: vec![("content-type".into(), content_type.into())],
                body: body.into(),
            }
        }

        pub fn header(mut self, name: &str, value: &str) -> Self {
            self.headers.push((name.into(), value.into()));
            self
        }
    }

    pub struct Server {
        pub base_url: String,
        requests: Arc<Mutex<Vec<Captured>>>,
    }

    impl Server {
        pub fn requests(&self) -> Vec<Captured> {
            self.requests.lock().unwrap().clone()
        }
    }

    /// Starts a server on a free local port.
    pub async fn start(answer: impl Fn(&Captured) -> Reply + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let answer = Arc::new(answer);
        let seen = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = seen.clone();
                let answer = answer.clone();
                tokio::spawn(async move {
                    while let Some(captured) = read_request(&mut socket).await {
                        let reply = answer(&captured);
                        seen.lock().unwrap().push(captured);
                        let mut out = format!("HTTP/1.1 {} X\r\n", reply.status).into_bytes();
                        for (name, value) in &reply.headers {
                            out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
                        }
                        out.extend_from_slice(
                            format!("content-length: {}\r\n\r\n", reply.body.len()).as_bytes(),
                        );
                        out.extend_from_slice(&reply.body);
                        if socket.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Server { base_url, requests }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Captured> {
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        let head_end = loop {
            if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                break at;
            }
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return None,
                Ok(n) => raw.extend_from_slice(&buf[..n]),
            }
        };
        let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
        let mut lines = head.split("\r\n");
        let mut start = lines.next().unwrap_or_default().split(' ');
        let method = start.next().unwrap_or_default().to_string();
        let target = start.next().unwrap_or_default().to_string();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let length = headers
            .iter()
            .find(|(k, _)| k == "content-length")
            .and_then(|(_, v)| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = raw[head_end + 4..].to_vec();
        while body.len() < length {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return None,
                Ok(n) => body.extend_from_slice(&buf[..n]),
            }
        }
        body.truncate(length);
        Some(Captured {
            method,
            target,
            headers,
            body,
        })
    }
}
