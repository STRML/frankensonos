#![forbid(unsafe_code)]

#[path = "support/spotify.rs"]
mod support;
use support::*;

#[test]
fn d1_d10_unconfigured_and_empty_cache_are_explicit() {
    let h = Harness::start(
        "daemon-empty",
        false,
        &Identity::fixed(Client::LoopbackHttp),
        Policy::default(),
        false,
    );
    let (code, body) = h.json("GET", "/auth/spotify/login");
    assert_eq!(code, 503);
    assert_eq!(body["code"], "SPOTIFY_NOT_CONFIGURED");
    assert!(
        body["hint"]
            .as_str()
            .unwrap()
            .contains("FSONOS_SPOTIFY_CLIENT_ID")
    );
    let status = h.json("GET", "/spotify/status").1;
    assert_eq!(
        status,
        json!({"configured":false,"signed_in":false,"reauthorize":false, "app_redirect_uri":"frankensonos://spotify-callback",
        "library":{"albums":0,"tracks":0,"synced_at":null},
        "sync":{"running":false,"done":0,"total":0,"error":null}})
    );
    for path in ["/spotify/albums", "/spotify/tracks"] {
        assert_eq!(h.json("GET", path), (200, json!({"total":0,"items":[]})));
    }
    assert_eq!(
        h.json("GET", "/spotify/albums/missing/tracks"),
        (200, json!([]))
    );
    h.no_secrets();
}

#[test]
fn d2_auth_rejects_unknown_and_serve_callers_before_policy() {
    for (name, identity, headers) in [
        (
            "daemon-unknown",
            Identity::fixed(Client::Unknown),
            Vec::new(),
        ),
        (
            "daemon-serve",
            Identity::behind_serve(Client::LoopbackHttp),
            vec![("Tailscale-User-Login", "owner@example.invalid")],
        ),
    ] {
        let h = Harness::start(name, true, &identity, Policy::default(), false);
        for path in [
            "/auth/spotify/login",
            "/auth/spotify/callback?state=x&code=x",
        ] {
            let (code, body, _) = h.request("GET", path, &headers);
            assert_eq!(code, 403);
            assert_eq!(
                serde_json::from_str::<Value>(&body).unwrap()["code"],
                "FORBIDDEN_NOT_LOOPBACK"
            );
        }
        assert!(
            h.fake
                .as_ref()
                .unwrap()
                .state
                .lock()
                .unwrap()
                .log
                .is_empty()
        );
        assert!(TokenCache::in_data_dir(&h.dir).load().unwrap().is_none());
    }
}

#[test]
fn d3_wrong_missing_and_replayed_state_store_nothing() {
    let h = Harness::local("daemon-state");
    for suffix in ["state=wrong&code=good-code", "code=good-code"] {
        let state = h.login();
        assert_eq!(
            h.request("GET", &format!("/auth/spotify/callback?{suffix}"), &[])
                .0,
            400
        );
        assert_eq!(h.callback(&state, "good-code").0, 400);
        assert!(TokenCache::in_data_dir(&h.dir).load().unwrap().is_none());
    }
    let state = h.login();
    assert_eq!(h.callback(&state, "good-code").0, 200);
    assert_eq!(h.callback(&state, "good-code").0, 400);
    assert_eq!(h.fake.as_ref().unwrap().state.lock().unwrap().log.len(), 1);
    h.no_secrets();
}

#[test]
fn d4_exchange_errors_are_safe_and_do_not_write_tokens() {
    for (name, status, body) in [
        (
            "daemon-grant",
            400,
            r#"{"error":"invalid_grant","error_description":"Expired code access-1 refresh-1"}"#,
        ),
        (
            "daemon-upstream",
            503,
            r#"{"error":"temporarily_unavailable"}"#,
        ),
    ] {
        let h = Harness::local(name);
        h.fake.as_ref().unwrap().state.lock().unwrap().token_error = Some((status, body.into()));
        let state = h.login();
        let (code, page, _) = h.callback(&state, "good-code");
        assert_eq!(code, 400);
        assert!(page.contains(if status == 400 {
            "invalid_grant"
        } else {
            "temporarily_unavailable"
        }));
        assert!(TokenCache::in_data_dir(&h.dir).load().unwrap().is_none());
        h.no_secrets();
    }
}

#[test]
fn d5_cache_write_failure_names_path_and_stays_signed_out() {
    let h = Harness::start(
        "daemon-cache-failure",
        true,
        &Identity::fixed(Client::LoopbackHttp),
        Policy::default(),
        true,
    );
    let state = h.login();
    let (code, page, _) = h.callback(&state, "good-code");
    assert_eq!(code, 400);
    assert!(page.contains("spotify-token.json"));
    assert_eq!(h.json("GET", "/spotify/status").1["signed_in"], false);
    h.no_secrets();
}

#[test]
fn d6_revoked_refresh_requires_login_and_has_no_retry_loop() {
    let h = Harness::local("daemon-refresh");
    h.authorize();
    {
        let mut fake = h.fake.as_ref().unwrap().state.lock().unwrap();
        fake.access = "revoked".into();
        fake.refresh = "revoked".into();
    }
    assert_eq!(h.json("POST", "/spotify/sync").0, 202);
    let status = h.wait(|s| !s["sync"]["running"].as_bool().unwrap());
    assert_eq!(status["reauthorize"], true);
    assert_eq!(status["sync"]["error"]["retryable"], false);
    assert_eq!(h.json("POST", "/spotify/sync").0, 409);
    assert_eq!(
        h.fake
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|l| l.ends_with("/api/token"))
            .count(),
        2
    );
    h.no_secrets();
}

#[test]
fn d7_d8_rate_limit_keeps_progress_and_sync_is_single_flight() {
    let h = Harness::local("daemon-rate-limit");
    h.authorize();
    h.fake
        .as_ref()
        .unwrap()
        .state
        .lock()
        .unwrap()
        .rate_limit_tracks_once = true;
    assert_eq!(h.json("POST", "/spotify/sync").0, 202);
    let limited = h.wait(|s| s["sync"]["error"]["retryable"] == true);
    assert_eq!(limited["sync"]["running"], true);
    assert!(limited["sync"]["done"].as_u64().unwrap() > 0);
    assert!(limited["sync"]["error"]["retry_at"].as_i64().unwrap() > 0);
    let (code, progress) = h.json("POST", "/spotify/sync");
    assert_eq!(code, 202);
    assert_eq!(progress["running"], true);
    assert_eq!(progress["done"], limited["sync"]["done"]);
    assert_eq!(progress["total"], limited["sync"]["total"]);
    let status = h.wait(|s| s["sync"]["running"] == false);
    assert_eq!(status["sync"]["error"], Value::Null);
    assert_eq!(
        h.fake
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .log
            .iter()
            .filter(|l| l.contains("/v1/me/tracks"))
            .count(),
        2
    );
    h.no_secrets();
}

#[test]
fn d9_d12_paged_library_lists_metadata_without_secrets() {
    let h = Harness::local("daemon-large");
    h.fake.as_ref().unwrap().state.lock().unwrap().liked_count = Some(2051);
    h.authorize();
    let start = Instant::now();
    assert_eq!(h.json("POST", "/spotify/sync").0, 202);
    assert!(start.elapsed() < Duration::from_secs(2));
    let status = h.wait(|s| s["sync"]["running"] == false);
    assert_eq!(status["sync"]["error"], Value::Null);
    assert_eq!(status["library"]["albums"], 2);
    assert_eq!(status["library"]["tracks"], 2051);
    assert!(status["library"]["synced_at"].as_i64().unwrap() > 0);
    assert_eq!(status["sync"]["done"], status["sync"]["total"]);
    let (code, albums) = h.json("GET", "/spotify/albums?q=Goldberg&limit=1");
    assert_eq!(code, 200);
    assert_eq!(albums["total"], 1);
    assert_eq!(
        albums["items"][0]["title"],
        "Bach: Goldberg Variations, BWV 988"
    );
    assert_eq!(
        albums["items"][0]["art_url"],
        "https://cdn.example.invalid/bach.jpg"
    );
    let id = albums["items"][0]["id"].as_str().unwrap();
    let tracks = h.json("GET", &format!("/spotify/albums/{id}/tracks")).1;
    assert_eq!(tracks.as_array().unwrap().len(), 3);
    assert_eq!(tracks[0]["disc"], 1);
    assert_eq!(tracks[0]["number"], 1);
    // The fixture track is 182_500 ms and the Spotify crate rounds durations up to whole seconds.
    assert_eq!(tracks[0]["duration_secs"], 183);
    assert!(tracks[0]["artists"].is_array());
    let liked = h.json("GET", "/spotify/tracks?offset=2050&limit=10").1;
    assert_eq!(liked["total"], 2051);
    assert_eq!(liked["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        liked["items"][0]["art_url"],
        "https://cdn.example.invalid/liked.jpg"
    );
    assert!(liked["items"][0]["album"].is_string());
    assert_eq!(h.json("GET", "/spotify/tracks?offset=bad").0, 422);
    h.no_secrets();
}

#[test]
fn d11_policy_names_the_new_operation_ids() {
    let policy = Policy::from_toml("[clients.unknown]\nallow = [\"spotify_status\"]\n").unwrap();
    let h = Harness::start(
        "daemon-policy",
        true,
        &Identity::fixed(Client::Unknown),
        policy,
        false,
    );
    assert_eq!(h.json("GET", "/spotify/status").0, 200);
    for (method, path, id) in [
        ("POST", "/spotify/sync", "spotify_sync"),
        ("GET", "/spotify/albums", "list_spotify_albums"),
        ("GET", "/spotify/tracks", "list_spotify_tracks"),
        (
            "GET",
            "/spotify/albums/x/tracks",
            "list_spotify_album_tracks",
        ),
    ] {
        let (code, body) = h.json(method, path);
        assert_eq!(code, 403);
        assert_eq!(body["code"], "POLICY_DENIED");
        assert!(body["detail"].as_str().unwrap().contains(id));
    }
}

#[test]
fn d4_network_failure_does_not_write_tokens() {
    let mut h = Harness::local("daemon-network");
    let state = h.login();
    h.fake.take().unwrap().stop();
    let (code, page, _) = h.callback(&state, "good-code");
    assert_eq!(code, 400);
    assert!(page.contains("Spotify request failed"));
    assert!(TokenCache::in_data_dir(&h.dir).load().unwrap().is_none());
    h.no_secrets();
}

#[test]
fn d8_exhausted_rate_limit_retains_retry_time_and_prior_cache() {
    let h = Harness::local("daemon-rate-exhausted");
    h.authorize();
    h.fake
        .as_ref()
        .unwrap()
        .state
        .lock()
        .unwrap()
        .rate_limit_albums = 4;
    assert_eq!(h.json("POST", "/spotify/sync").0, 202);
    let status = h.wait(|s| s["sync"]["running"] == false);
    assert_eq!(status["sync"]["error"]["retryable"], true);
    assert!(status["sync"]["error"]["retry_at"].as_i64().unwrap() > 0);
    assert_eq!(status["library"]["synced_at"], Value::Null);
    assert_eq!(h.json("GET", "/spotify/albums").1["items"], json!([]));
    h.no_secrets();
}

#[test]
fn resync_removes_unliked_tracks_from_browse_membership() {
    let h = Harness::local("daemon-unlike");
    h.authorize();
    assert_eq!(h.json("POST", "/spotify/sync").0, 202);
    h.wait(|s| s["sync"]["running"] == false);
    assert_eq!(h.json("GET", "/spotify/tracks").1["total"], 1);
    h.fake.as_ref().unwrap().state.lock().unwrap().liked_tracks =
        Some(r#"{"items":[],"next":null,"total":0}"#.into());
    assert_eq!(h.json("POST", "/spotify/sync").0, 202);
    h.wait(|s| s["sync"]["running"] == false);
    assert_eq!(
        h.json("GET", "/spotify/tracks").1,
        json!({"total":0,"items":[]})
    );
    h.no_secrets();
}
