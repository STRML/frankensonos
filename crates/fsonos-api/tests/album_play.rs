//! P1–P9 through HTTP, fake Spotify and the same LAN transport as `fsonos sim`.
#![forbid(unsafe_code)]

#[path = "support/spotify.rs"]
mod support;
use fsonos_core::store::{
    LibraryEntry, LibraryOrigin, MemStore, SpotifyAlbum, SpotifyCache, Store,
};
use fsonos_proto::{ProtoError, Transport, content, control};
use fsonos_sim::{SimHandle, SimHousehold, SimLan};
use fsonos_types::{Track, TransportState};
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use support::*;

const ALBUM: &str = "spotify:album:FakeAlbum0000000000001";
const OLD: &str = "x-rincon-mp3radio://stream.example.invalid/old.mp3";

struct FailAdd {
    lan: SimLan,
    nth: Arc<AtomicUsize>,
    count: AtomicUsize,
}
impl Transport for FailAdd {
    fn soap_post(
        &self,
        host: IpAddr,
        path: &str,
        action: &str,
        body: &str,
    ) -> Result<String, ProtoError> {
        if action.trim_matches('"').ends_with("#AddURIToQueue")
            && self.count.fetch_add(1, Ordering::SeqCst) + 1 == self.nth.load(Ordering::SeqCst)
        {
            return Err(ProtoError::SoapFault {
                code: 701,
                reason: "injected nth add failure".into(),
            });
        }
        self.lan.soap_post(host, path, action, body)
    }
}

fn cached(count: u32) -> MemStore {
    let mut store = MemStore::default();
    // Reverse insertion order and two discs catch cache-order and disc-order mistakes.
    let rows: Vec<_> = (1..=count)
        .rev()
        .map(|n| LibraryEntry {
            track: Track {
                title: format!("Movement {n}"),
                artist: Some("Test Artist".into()),
                album: Some("Test Album".into()),
                source_uri: format!("spotify:track:AlbumTrack{n:012}"),
                uri: None,
                duration_secs: Some(180),
            },
            is_classical: true,
            added: 1,
            album_uri: Some(ALBUM.into()),
            album_artists: None,
            origin: LibraryOrigin::SavedAlbum,
            disc_number: Some((n - 1) / 50 + 1),
            track_number: Some((n - 1) % 50 + 1),
            work_key: None,
        })
        .collect();
    store.upsert_library(&rows).unwrap();
    store
        .save_spotify_cache(&SpotifyCache {
            albums: vec![SpotifyAlbum {
                id: ALBUM.strip_prefix("spotify:album:").unwrap().into(),
                title: "Test Album".into(),
                artist: "Test Artist".into(),
                year: None,
                tracks: count,
                uri: ALBUM.into(),
                art_url: None,
                saved: true,
            }],
            track_uris: rows.iter().map(|r| r.track.source_uri.clone()).collect(),
            ..SpotifyCache::default()
        })
        .unwrap();
    store
}

struct AlbumHarness {
    http: Harness,
    sim: SimHandle,
    fail: Arc<AtomicUsize>,
}
impl AlbumHarness {
    fn start(name: &str, configured: bool, store: MemStore, denied: bool) -> Self {
        let sim = SimHousehold::standard().spawn().unwrap();
        let survey_lan = sim.lan();
        let fail = Arc::new(AtomicUsize::new(0));
        let transport = FailAdd {
            lan: sim.lan(),
            nth: fail.clone(),
            count: AtomicUsize::new(0),
        };
        let identity = Identity::fixed(if denied {
            Client::Unknown
        } else {
            Client::LoopbackHttp
        });
        let policy = if denied {
            Policy::from_toml("[clients.unknown]\nallow = [\"list_zones\"]\n").unwrap()
        } else {
            Policy::default()
        };
        let http = Harness::start_surface(name, configured, &identity, false, |spotify| {
            Surface::new(
                Box::new(transport),
                Box::new(move |_| {
                    Ok(fsonos_core::inventory::survey(
                        &survey_lan,
                        &[],
                        Duration::from_millis(500),
                    )?
                    .households)
                }),
                policy,
                Box::new(SystemClock),
            )
            .with_action_log(Box::new(store), "test")
            .with_spotify(spotify)
        });
        let h = Self { http, sim, fail };
        control::add_uri_to_queue(&h.sim.lan(), h.ip("Living Room"), OLD, "", false).unwrap();
        h
    }
    fn ip(&self, room: &str) -> IpAddr {
        self.sim.player(room).unwrap().ip
    }
    fn play(&self, room: &str, uri: &str) -> (u16, Value) {
        let (status, body, _) = self.http.request_body(
            "POST",
            "/play",
            &[],
            &json!({"zone":room,"source_uri":uri}).to_string(),
        );
        (status, serde_json::from_str(&body).unwrap())
    }
    fn queue(&self) -> Vec<String> {
        content::browse_all(&self.sim.lan(), self.ip("Living Room"), "Q:0")
            .unwrap()
            .into_iter()
            .map(|o| o.res.unwrap().uri)
            .collect()
    }
    fn untouched(&self) {
        assert_eq!(self.queue(), [OLD]);
        assert!(
            !self
                .sim
                .soap_log()
                .iter()
                .any(|l| l.action == "RemoveAllTracksFromQueue")
        );
    }
    fn playing(&self, count: usize) {
        assert_eq!(self.queue().len(), count);
        assert_eq!(
            control::get_position_info(&self.sim.lan(), self.ip("Living Room"))
                .unwrap()
                .track,
            1
        );
        assert_eq!(
            control::get_transport_info(&self.sim.lan(), self.ip("Living Room"))
                .unwrap()
                .state,
            TransportState::Playing
        );
    }
}

#[test]
fn p1_cached_album_clears_orders_and_plays_from_one_with_track_didl() {
    let h = AlbumHarness::start("album-p1", false, cached(3), false);
    let (code, body) = h.play("Living Room", ALBUM);
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["done"], "playing album Test Album (3 tracks)");
    h.playing(3);
    let log = h.sim.soap_log();
    let adds: Vec<_> = log
        .iter()
        .filter(|l| {
            l.action == "AddURIToQueue"
                && l.args
                    .iter()
                    .any(|(k, v)| k == "EnqueuedURI" && v.starts_with("spotify%3a"))
        })
        .collect();
    assert_eq!(adds.len(), 3);
    let clear = log
        .iter()
        .position(|l| l.action == "RemoveAllTracksFromQueue")
        .unwrap();
    assert!(log[clear + 1..].iter().any(|l| l.action == "AddURIToQueue"));
    for (n, add) in adds.iter().enumerate() {
        let arg = |key: &str| add.args.iter().find(|(k, _)| k == key).unwrap().1.as_str();
        assert_eq!(
            arg("EnqueuedURI"),
            format!("spotify%3atrack%3aAlbumTrack{:012}", n + 1)
        );
        let didl = arg("EnqueuedURIMetaData").to_string();
        let (code, body, _) = h.http.request_body("POST", "/play", &[], &json!({"zone":"Living Room", "source_uri":format!("spotify:track:AlbumTrack{:012}", n+1), "title":format!("Movement {}",n+1)}).to_string());
        assert_eq!(code, 200, "{body}");
        let log = h.sim.soap_log();
        let track = log
            .iter()
            .rev()
            .find(|l| l.action == "SetAVTransportURI")
            .unwrap();
        assert_eq!(
            track
                .args
                .iter()
                .find(|(k, _)| k == "CurrentURIMetaData")
                .unwrap()
                .1,
            didl
        );
    }
}

#[test]
fn p2_uncached_signed_out_or_unconfigured_preserves_queue() {
    for configured in [false, true] {
        let h = AlbumHarness::start("album-p2", configured, MemStore::default(), false);
        let (status, body) = h.play("Living Room", ALBUM);
        assert!((400..500).contains(&status), "{status}: {body}");
        assert_eq!(
            body["code"],
            if configured {
                "SPOTIFY_AUTH_REQUIRED"
            } else {
                "SPOTIFY_NOT_CONFIGURED"
            }
        );
        assert!(!body["hint"].as_str().unwrap().is_empty());
        h.untouched();
    }
}

#[test]
fn p3_unknown_album_fetch_fails_before_clearing() {
    let h = AlbumHarness::start("album-p3", true, MemStore::default(), false);
    h.http.authorize();
    let (status, body) = h.play("Living Room", "spotify:album:UnknownAlbum0000000001");
    assert_eq!(status, 404, "{body}");
    assert!(
        body["detail"]
            .as_str()
            .unwrap()
            .contains("album not in your synced library")
    );
    assert!(
        body["hint"]
            .as_str()
            .unwrap()
            .contains("sync it or play one of its tracks")
    );
    h.untouched();
}

#[test]
fn p4_nth_enqueue_failure_reports_confirmed_partial_count() {
    let h = AlbumHarness::start("album-p4", false, cached(3), false);
    h.fail.store(2, Ordering::SeqCst);
    let (status, body) = h.play("Living Room", ALBUM);
    assert_eq!(status, 502, "{body}");
    assert_eq!(body["code"], "UPNP_FAULT");
    assert_eq!(body["retryable"], false);
    assert!(
        body["detail"]
            .as_str()
            .unwrap()
            .contains("1 of 3 tracks added")
    );
    assert_eq!(h.queue().len(), 1);
    assert!(!h.sim.soap_log().iter().any(|l| l.action == "Play"));
}

#[test]
fn p5_missing_favorite_preserves_queue_with_track_error() {
    let h = AlbumHarness::start("album-p5", false, cached(3), false);
    h.sim.retain_favorites(2, |_| false).unwrap();
    let (status, body) = h.play("Living Room", ALBUM);
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "RENDER_PARAMS_MISSING");
    assert!(body["hint"].as_str().unwrap().contains("My Sonos"));
    h.untouched();
}

#[test]
fn p6_unsupported_kinds_name_track_and_album() {
    let h = AlbumHarness::start("album-p6", false, cached(3), false);
    for kind in ["playlist", "artist", "show", "episode"] {
        let (status, body) = h.play(
            "Living Room",
            &format!("spotify:{kind}:FakeItem00000000000001"),
        );
        assert_eq!(status, 501, "{body}");
        assert_eq!(body["code"], "NOT_IMPLEMENTED");
        let hint = body["hint"].as_str().unwrap();
        assert!(hint.contains("track") && hint.contains("album"), "{hint}");
    }
    h.untouched();
}

#[test]
fn p7_play_policy_denies_before_queue_changes() {
    let h = AlbumHarness::start("album-p7", true, cached(3), true);
    let (status, body) = h.play("Living Room", ALBUM);
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["code"], "POLICY_DENIED");
    assert!(body["detail"].as_str().unwrap().contains("play"));
    h.untouched();
    assert!(
        h.http
            .fake
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .log
            .is_empty()
    );
}

#[test]
fn p8_large_album_caps_and_reports_first_hundred() {
    let h = AlbumHarness::start("album-p8", false, cached(101), false);
    let (status, body) = h.play("Living Room", ALBUM);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["done"],
        "playing album Test Album (100 tracks; first 100 of 101)"
    );
    h.playing(100);
    let queue = h.queue();
    assert!(queue[0].contains("AlbumTrack000000000001"));
    assert!(queue[99].contains("AlbumTrack000000000100"));
}

#[test]
fn p9_grouped_room_builds_coordinator_queue_and_canonicalizes_link() {
    let h = AlbumHarness::start("album-p9", false, cached(3), false);
    let coordinator = fsonos_types::PlayerId(h.sim.player("Living Room").unwrap().uuid.clone());
    control::join_group(&h.sim.lan(), h.ip("Bedroom"), &coordinator).unwrap();
    let (status, body) = h.play(
        "Bedroom",
        "https://open.spotify.com/album/FakeAlbum0000000000001?si=test",
    );
    assert_eq!(status, 200, "{body}");
    h.playing(3);
    assert!(
        h.sim
            .soap_log()
            .iter()
            .filter(|l| l.action == "RemoveAllTracksFromQueue" || l.action == "AddURIToQueue")
            .all(|l| l.room == "Living Room")
    );
}

#[test]
fn uncached_album_uses_existing_paged_spotify_client_before_queueing() {
    let h = AlbumHarness::start("album-fetch", true, MemStore::default(), false);
    h.http.authorize();
    let (status, body) = h.play("Living Room", "spotify:album:FakeAlbum0000000000009");
    assert_eq!(status, 200, "{body}");
    h.playing(6);
    assert!(
        h.http
            .fake
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .log
            .iter()
            .any(|l| l.contains("offset=3"))
    );
    h.http.no_secrets();
}
