#![allow(unused_imports, dead_code)]

pub use asupersync::Cx;
pub use asupersync::http::Client as HttpClient;
pub use fastapi::{ServerConfig, TcpServer};
pub use fsonos_api::spotify::Spotify;
pub use fsonos_api::{Identity, Surface, WebPolicy};
pub use fsonos_core::clock::SystemClock;
pub use fsonos_core::policy::{Client, Policy};
pub use fsonos_core::store::SqliteStore;
pub use fsonos_spotify::client::TokenCache;
pub use fsonos_spotify::fake_spotify::{FakeSpotify, config, query_param, runtime, scratch_dir};
pub use serde_json::{Value, json};
pub use std::io::{self, Write};
pub use std::path::PathBuf;
pub use std::sync::{Arc, Mutex, OnceLock, mpsc};
pub use std::thread;
pub use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);
impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn logs() -> Logs {
    static LOGS: OnceLock<Logs> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = Logs::default();
        let writer = logs.clone();
        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_writer(move || writer.clone())
                .without_time()
                .finish(),
        )
        .unwrap();
        logs
    })
    .clone()
}

pub struct NoLan;
impl fsonos_proto::Transport for NoLan {
    fn soap_post(
        &self,
        _: std::net::IpAddr,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<String, fsonos_proto::ProtoError> {
        Err(fsonos_proto::ProtoError::NotWired("test has no speakers"))
    }
}

pub struct Harness {
    pub fake: Option<FakeSpotify>,
    pub dir: PathBuf,
    pub base: String,
    server: Arc<TcpServer>,
    thread: Option<thread::JoinHandle<()>>,
    pub bodies: Mutex<Vec<String>>,
    pub logs: Logs,
}
impl Harness {
    pub fn start(
        name: &str,
        configured: bool,
        identity: &Identity,
        policy: Policy,
        unwritable: bool,
    ) -> Self {
        let logs = logs();
        let fake = FakeSpotify::start();
        let dir = scratch_dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        if unwritable {
            std::fs::create_dir_all(dir.join("auth/spotify-token.json.tmp")).unwrap();
        }
        let spotify = Spotify::new(
            configured.then(config),
            &dir,
            fake.endpoints(),
            Some(format!("http://{}", fake.addr)),
            "frankensonos://spotify-callback".into(),
        )
        .unwrap();
        let surface = Arc::new(
            Surface::new(
                Box::new(NoLan),
                Box::new(|_| Ok(Vec::new())),
                policy,
                Box::new(SystemClock),
            )
            .with_action_log(Box::new(SqliteStore::open_in_memory().unwrap()), "test")
            .with_spotify(spotify),
        );
        let web = WebPolicy::for_listener("127.0.0.1:0".parse().unwrap(), &[]);
        let app = Arc::new(fsonos_api::app(&surface, identity, &web));
        let server = Arc::new(TcpServer::new(
            ServerConfig::new("127.0.0.1:0").with_allowed_hosts(web.hosts().to_vec()),
        ));
        let serving = server.clone();
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            runtime().block_on(async move {
                let cx = Cx::current().unwrap();
                let listener = asupersync::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .unwrap();
                tx.send(listener.local_addr().unwrap()).unwrap();
                let _ = serving.serve_on_app_concurrent(&cx, listener, app).await;
            });
        });
        let addr = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        Self {
            fake: Some(fake),
            dir,
            base: format!("http://{addr}"),
            server,
            thread: Some(thread),
            bodies: Mutex::default(),
            logs,
        }
    }
    pub fn local(name: &str) -> Self {
        Self::start(
            name,
            true,
            &Identity::fixed(Client::LoopbackHttp),
            Policy::default(),
            false,
        )
    }
    pub fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> (u16, String, String) {
        self.request_body(method, path, headers, "{}")
    }
    pub fn request_body(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (u16, String, String) {
        let url = format!("{}{path}", self.base);
        let response = runtime().block_on(async {
            let cx = Cx::current().unwrap();
            // Login answers 302 to the Spotify authorize URL; the test reads that redirect, it must not follow it.
            let client = HttpClient::builder().no_redirects().build();
            let mut req = if method == "POST" {
                client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .body(body.to_string())
            } else {
                client.get(&url)
            };
            for (key, value) in headers {
                req = req.header(*key, *value);
            }
            req.timeout(Duration::from_secs(35))
                .send(&cx)
                .await
                .unwrap()
        });
        let location = response
            .header_value("location")
            .unwrap_or_default()
            .to_string();
        let body = String::from_utf8(response.body).unwrap();
        self.bodies
            .lock()
            .unwrap()
            .extend([body.clone(), location.clone()]);
        (response.status, body, location)
    }
    pub fn json(&self, method: &str, path: &str) -> (u16, Value) {
        let (status, body, _) = self.request(method, path, &[]);
        (status, serde_json::from_str(&body).unwrap())
    }
    pub fn login(&self) -> String {
        let (status, body, location) = self.request("GET", "/auth/spotify/login", &[]);
        assert_eq!(
            status,
            302,
            "login answered: {body} (base {}, fake {})",
            self.base,
            self.fake.as_ref().unwrap().addr
        );
        let fake = self.fake.as_ref().unwrap();
        assert!(location.starts_with(&format!("http://{}/authorize?", fake.addr)));
        fake.state.lock().unwrap().expected_challenge =
            Some(query_param(&location, "code_challenge"));
        query_param(&location, "state")
    }
    pub fn callback(&self, state: &str, code: &str) -> (u16, String, String) {
        self.request(
            "GET",
            &format!("/auth/spotify/callback?state={state}&code={code}"),
            &[],
        )
    }
    pub fn authorize(&self) {
        let state = self.login();
        let (status, body, _) = self.callback(&state, "good-code");
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("Signed in. You can close this tab."));
        assert_eq!(self.json("GET", "/spotify/status").1["signed_in"], true);
    }
    pub fn wait(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let (code, status) = self.json("GET", "/spotify/status");
            assert_eq!(code, 200);
            if predicate(&status) {
                return status;
            }
            assert!(Instant::now() < deadline, "{status}");
            thread::sleep(Duration::from_millis(30));
        }
    }
    pub fn no_secrets(&self) {
        self.no_secrets_in(&[]);
    }
    pub fn no_secrets_in(&self, secrets: &[&str]) {
        let (status, actions) = self.json("GET", "/actions");
        assert_eq!(status, 200);
        let actions = actions.to_string();
        let logs = String::from_utf8(self.logs.0.lock().unwrap().clone()).unwrap();
        let bodies = self.bodies.lock().unwrap().join("\n");
        for token in ["access-1", "refresh-1", "access-r1", "refresh-r1"]
            .into_iter()
            .chain(secrets.iter().copied())
        {
            for text in [&actions, &logs, &bodies] {
                assert!(!text.contains(token), "secret escaped");
            }
        }
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.server.shutdown();
        let addr = self.base.strip_prefix("http://").unwrap();
        drop(std::net::TcpStream::connect(addr));
        self.thread.take().unwrap().join().unwrap();
        if let Some(fake) = self.fake.take() {
            fake.stop();
        }
    }
}
