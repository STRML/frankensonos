//! Real daemon sign-in and sync against fake Spotify, followed by DJ playback.
#![forbid(unsafe_code)]

mod e2e;

use e2e::{Scenario, http};
use fsonos_proto::control::get_transport_info;
use fsonos_sim::SimHousehold;
use fsonos_spotify::fake_spotify::{CLIENT_ID, FakeSpotify, REDIRECT, query_param};
use fsonos_types::TransportState;
use serde_json::Value;
use std::time::{Duration, Instant};

#[test]
fn sign_in_and_sync_make_the_dj_work_without_a_restart() {
    let fake = FakeSpotify::start();
    let mut s = Scenario::start("spotify-dj");
    s.sim(SimHousehold::standard());
    let accounts = format!("http://{}", fake.addr);
    let api = fake.endpoints().api;
    let args = [
        "serve",
        "--http",
        "127.0.0.1:0",
        "--mcp-http",
        "127.0.0.1:0",
        "--spotify-client-id",
        CLIENT_ID,
        "--spotify-redirect-uri",
        REDIRECT,
        "--spotify-accounts-url",
        &accounts,
        "--spotify-api-url",
        &api,
    ];
    let mut daemon = s.spawn("serve", &args);
    let ready = daemon
        .wait_line("fsonos serve: ready", Duration::from_secs(20))
        .unwrap();
    let addr = ready
        .split_whitespace()
        .find_map(|w| w.strip_prefix("http=http://"))
        .unwrap();
    daemon
        .wait_line("fsonos serve: live", Duration::from_secs(20))
        .unwrap();
    let mut bodies = Vec::new();
    let mut request = |method, path: &str, body: &str| {
        let (code, headers, text) = http(
            addr,
            method,
            path,
            &[("Content-Type", "application/json")],
            body,
        )
        .unwrap();
        bodies.push(text.clone());
        (code, headers, text)
    };
    let before = request("POST", "/dj/start", r#"{"zone":"Living Room"}"#);
    assert_ne!(before.0, 200);
    let (code, headers, _) = request("GET", "/auth/spotify/login", "");
    assert_eq!(code, 302);
    let url = &headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("location"))
        .unwrap()
        .1;
    fake.state.lock().unwrap().expected_challenge = Some(query_param(url, "code_challenge"));
    let state = query_param(url, "state");
    assert_eq!(
        request(
            "GET",
            &format!("/auth/spotify/callback?state={state}&code=good-code"),
            ""
        )
        .0,
        200
    );
    assert_eq!(request("POST", "/spotify/sync", "{}").0, 202);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (_, _, text) = request("GET", "/spotify/status", "");
        let status: Value = serde_json::from_str(&text).unwrap();
        if status["sync"]["running"] == false {
            assert_eq!(status["sync"]["error"], Value::Null);
            assert_eq!(status["signed_in"], true);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    let (code, _, text) = request("POST", "/dj/start", r#"{"zone":"Living Room"}"#);
    assert_eq!(code, 200, "{text}");
    let transport = get_transport_info(&s.lan(), s.ip("Living Room")).unwrap();
    assert_eq!(transport.state, TransportState::Playing);
    request("GET", "/actions", "");
    let stopped = daemon.interrupt(Duration::from_secs(10));
    assert_eq!(stopped, Some(0));
    for token in ["access-1", "refresh-1", "access-r1", "refresh-r1"] {
        assert!(!bodies.join("\n").contains(token));
        assert!(!daemon.seen.join("\n").contains(token));
    }
    s.check(
        "spotify-dj",
        "http+sim",
        "sign-in and sync let the existing DJ play without restarting",
        true,
        text,
    );
    s.finish();
    fake.stop();
}
