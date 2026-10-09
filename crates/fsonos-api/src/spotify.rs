//! Daemon Spotify authorization and background library sync.

use asupersync::Cx;
use asupersync::http::Client;
use asupersync::runtime::{RuntimeBuilder, reactor::create_reactor};
use asupersync::time::{sleep, wall_now};
use fastapi::{JsonSchema, Response, ResponseBody, StatusCode, fastapi_openapi};
use fsonos_core::store::{LibraryEntry, SpotifyAlbum, SpotifyCache};
use fsonos_spotify::SpotifyError;
use fsonos_spotify::cache::apply_library_read;
use fsonos_spotify::client::{AUTHORIZE_URL, Endpoints, Pkce, SpotifyConfig, TokenCache};
use fsonos_spotify::library::{LibraryRead, split_artists};
use fsonos_spotify::session::{PendingAuthorization, Session};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::failure::{ErrorCode, Failure};
use crate::surface::SharedStore;

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct SyncDto {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub error: Option<SyncErrorDto>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncErrorDto {
    pub detail: String,
    pub retryable: bool,
    pub retry_at: Option<i64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryDto {
    pub albums: usize,
    pub tracks: usize,
    pub synced_at: Option<i64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusDto {
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub app_redirect_uri: String,
    pub signed_in: bool,
    pub reauthorize: bool,
    pub library: LibraryDto,
    pub sync: SyncDto,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AlbumDto {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub tracks: u32,
    pub uri: String,
    pub art_url: Option<String>,
}

impl From<SpotifyAlbum> for AlbumDto {
    fn from(album: SpotifyAlbum) -> Self {
        Self {
            id: album.id,
            title: album.title,
            artist: album.artist,
            year: album.year,
            tracks: album.tracks,
            uri: album.uri,
            art_url: album.art_url,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TrackDto {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub uri: String,
    pub duration_secs: Option<u32>,
    pub disc: Option<u32>,
    pub number: Option<u32>,
}
impl From<&LibraryEntry> for TrackDto {
    fn from(row: &LibraryEntry) -> Self {
        Self {
            id: row
                .track
                .source_uri
                .strip_prefix("spotify:track:")
                .unwrap_or_default()
                .into(),
            title: row.track.title.clone(),
            artists: row
                .track
                .artist
                .as_deref()
                .map(split_artists)
                .unwrap_or_default(),
            uri: row.track.source_uri.clone(),
            duration_secs: row.track.duration_secs,
            disc: row.disc_number,
            number: row.track_number,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct LikedTrackDto {
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub uri: String,
    pub duration_secs: Option<u32>,
    pub disc: Option<u32>,
    pub number: Option<u32>,
    pub album: Option<String>,
    pub art_url: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AlbumsDto {
    pub total: usize,
    pub items: Vec<AlbumDto>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TracksDto {
    pub total: usize,
    pub items: Vec<LikedTrackDto>,
}

struct TimedAuthorization {
    pending: PendingAuthorization,
    at: Instant,
}

#[derive(Default)]
struct State {
    pending: Option<TimedAuthorization>,
    signed_in: bool,
    reauthorize: bool,
    authorizing: bool,
    sync: SyncDto,
}

/// One authorization state shared by all daemon listeners. Tokens stay in
/// Session and the private token cache, never in DTOs or action entries.
pub struct Spotify {
    config: Option<SpotifyConfig>,
    cache: TokenCache,
    endpoints: Endpoints,
    accounts: Option<String>,
    app_redirect_uri: String,
    state: Mutex<State>,
}

pub const DEFAULT_APP_REDIRECT: &str = "frankensonos://spotify-callback";

#[derive(Deserialize, JsonSchema)]
pub struct ExchangeRequest {
    pub code: String,
    pub code_verifier: String,
    pub redirect_uri: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ExchangeDto {
    pub signed_in: bool,
}

struct Authorizing<'a>(&'a Spotify);
impl Drop for Authorizing<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.state.lock() {
            state.authorizing = false;
        }
    }
}

fn validate_exchange(input: &ExchangeRequest, redirect: &str) -> Result<Pkce, Failure> {
    if input.redirect_uri != redirect {
        return Err(Failure::new(
            ErrorCode::SpotifyRedirectNotAllowed,
            "Spotify app redirect is not allowed.",
        ));
    }
    if input.code.is_empty() || input.code.len() > 512 {
        return Err(Failure::invalid("code must contain 1 to 512 bytes"));
    }
    Pkce::from_verifier(input.code_verifier.clone()).map_err(|_| {
        Failure::invalid("code_verifier must be 43 to 128 unreserved ASCII characters")
    })
}

impl Spotify {
    pub fn new(
        config: Option<SpotifyConfig>,
        data_dir: &Path,
        endpoints: Endpoints,
        accounts: Option<String>,
        app_redirect_uri: String,
    ) -> Result<Arc<Self>, Failure> {
        if let Some(config) = &config {
            config
                .validate()
                .map_err(|_| Failure::invalid("invalid Spotify configuration"))?;
        }
        let cache = TokenCache::in_data_dir(data_dir);
        let signed_in = config.is_some() && cache.load().ok().flatten().is_some();
        Ok(Arc::new(Self {
            config,
            cache,
            endpoints,
            accounts,
            app_redirect_uri,
            state: Mutex::new(State {
                signed_in,
                ..State::default()
            }),
        }))
    }

    pub fn login(&self) -> Result<String, Failure> {
        let config = self.config.as_ref().ok_or_else(not_configured)?;
        let mut state = self.state.lock().map_err(|_| internal())?;
        if state.authorizing || state.sync.running {
            return Err(busy());
        }
        let pending = PendingAuthorization::begin(config).map_err(|_| internal())?;
        let url = self.accounts.as_ref().map_or_else(
            || pending.url().to_string(),
            |accounts| {
                pending.url().replacen(
                    AUTHORIZE_URL,
                    &format!("{}/authorize", accounts.trim_end_matches('/')),
                    1,
                )
            },
        );
        state.pending = Some(TimedAuthorization {
            pending,
            at: Instant::now(),
        });
        Ok(url)
    }

    pub fn callback(self: &Arc<Self>, callback: String) -> Response {
        let pending = {
            let Ok(mut state) = self.state.lock() else {
                return internal().http_response();
            };
            let pending = match take_authorization(&mut state.pending, Instant::now()) {
                Ok(pending) => pending,
                Err(why) => return html(400, why),
            };
            if state.authorizing || state.sync.running {
                return html(400, "Spotify is busy; start sign-in again.");
            }
            state.authorizing = true;
            pending
        };
        let this = Arc::clone(self);
        let result = thread::Builder::new()
            .name("fsonos-spotify-auth".into())
            .spawn(move || {
                this.run_session(async |session, cx| {
                    session
                        .complete_authorization(cx, &pending, &callback)
                        .await
                })
            })
            .map_err(|_| "Could not start Spotify sign-in.".to_string())
            .and_then(|worker| {
                worker
                    .join()
                    .map_err(|_| "Spotify sign-in worker failed.".to_string())
            })
            .and_then(|result| result.map_err(|err| safe_error(&err, self.cache.path())));
        let Ok(mut state) = self.state.lock() else {
            return internal().http_response();
        };
        state.authorizing = false;
        match result {
            Ok(()) => {
                state.signed_in = true;
                state.reauthorize = false;
                state.sync.error = None;
                html(200, "Signed in. You can close this tab.")
            }
            Err(why) => {
                state.signed_in = false;
                html(400, &why)
            }
        }
    }

    async fn exchange(&self, cx: &Cx, input: ExchangeRequest, store: SharedStore) -> Response {
        let Some(config) = self.config.clone() else {
            return not_configured().http_response();
        };
        let pkce = match validate_exchange(&input, &self.app_redirect_uri) {
            Ok(pkce) => pkce,
            Err(err) => return crate::failure::http_error(&err, 400, false),
        };
        {
            let Ok(mut state) = self.state.lock() else {
                return internal().http_response();
            };
            if state
                .pending
                .as_ref()
                .is_some_and(|p| p.at.elapsed() >= Duration::from_secs(600))
            {
                state.pending = None;
            }
            if state.authorizing || state.sync.running || state.pending.is_some() {
                return crate::failure::http_error(&busy(), 409, true);
            }
            state.authorizing = true;
        }
        let _authorizing = Authorizing(self);
        let result = async {
            let mut session =
                Session::open(config, self.cache.clone(), Client::default_for_runtime(cx))?
                    .with_endpoints(self.endpoints.clone());
            session
                .exchange_code(cx, &input.code, &pkce, &input.redirect_uri)
                .await?;
            store
                .lock()
                .map_err(|_| SpotifyError::Config("store unavailable".into()))?
                .clear_spotify_library()?;
            Ok::<_, SpotifyError>(())
        }
        .await;
        let Ok(mut state) = self.state.lock() else {
            return internal().http_response();
        };
        match result {
            Ok(()) => {
                state.signed_in = true;
                state.reauthorize = false;
                state.sync = SyncDto::default();
                Response::json(&ExchangeDto { signed_in: true })
                    .expect("ExchangeDto serializes")
                    .header("cache-control", b"no-store".to_vec())
            }
            Err(err) => {
                if matches!(err, SpotifyError::Io(_) | SpotifyError::Store(_)) {
                    state.signed_in = false;
                }
                let (status, retryable) = match err {
                    SpotifyError::Auth(_) => (400, false),
                    SpotifyError::Http(_) | SpotifyError::Api { .. } | SpotifyError::Decode(_) => {
                        (502, true)
                    }
                    _ => (500, false),
                };
                crate::failure::http_error(
                    &Failure::new(
                        ErrorCode::SpotifyAuthRequired,
                        safe_error(&err, self.cache.path()),
                    ),
                    status,
                    retryable,
                )
            }
        }
    }

    fn run_session<T>(
        &self,
        work: impl for<'a> AsyncFnOnce(&'a mut Session, &'a Cx) -> Result<T, SpotifyError>,
    ) -> Result<T, SpotifyError> {
        let rt = RuntimeBuilder::current_thread()
            .with_reactor(create_reactor().map_err(|e| SpotifyError::Http(e.to_string()))?)
            .blocking_threads(0, 4)
            .build()
            .map_err(|e| SpotifyError::Http(e.to_string()))?;
        rt.block_on(async {
            let cx = Cx::current()
                .ok_or_else(|| SpotifyError::Http("runtime context missing".into()))?;
            let mut session = Session::open(
                self.config
                    .clone()
                    .ok_or_else(|| SpotifyError::Config("not configured".into()))?,
                self.cache.clone(),
                Client::default_for_runtime(&cx),
            )?
            .with_endpoints(self.endpoints.clone());
            work(&mut session, &cx).await
        })
    }

    async fn read_into_store(
        &self,
        session: &mut Session,
        cx: &Cx,
        store: &SharedStore,
    ) -> Result<(), SpotifyError> {
        let mut read = LibraryRead::new(session.endpoints());
        while let Some(url) = read.next_url().map(str::to_string) {
            let body = session
                .get_observed(cx, &url, |secs| {
                    if let Ok(mut state) = self.state.lock() {
                        state.sync.error = Some(SyncErrorDto {
                            detail: "Spotify rate limit; waiting for Retry-After.".into(),
                            retryable: true,
                            retry_at: Some(
                                unix_now().saturating_add(i64::try_from(secs).unwrap_or(i64::MAX)),
                            ),
                        });
                    }
                })
                .await?;
            read.ingest(&body)?;
            if let Ok(mut state) = self.state.lock() {
                (state.sync.done, state.sync.total) = read.progress();
                state.sync.error = None;
            }
        }
        let mut cache = read.browse_cache();
        cache.synced_at = Some(unix_now());
        let items = read.into_items();
        let mut store = store
            .lock()
            .map_err(|_| SpotifyError::Config("store unavailable".into()))?;
        apply_library_read(&mut **store, &items)?;
        store.save_spotify_cache(&cache)?;
        Ok(())
    }

    pub fn status(&self, cache: &SpotifyCache) -> Result<StatusDto, Failure> {
        let state = self.state.lock().map_err(|_| internal())?;
        Ok(StatusDto {
            configured: self.config.is_some(),
            client_id: self.config.as_ref().map(|config| config.client_id.clone()),
            app_redirect_uri: self.app_redirect_uri.clone(),
            signed_in: state.signed_in,
            reauthorize: state.reauthorize,
            library: library_status(cache),
            sync: state.sync.clone(),
        })
    }

    pub(crate) fn sync(self: &Arc<Self>, store: SharedStore) -> Result<SyncDto, Failure> {
        let mut state = self.state.lock().map_err(|_| internal())?;
        if state.sync.running {
            return Ok(state.sync.clone());
        }
        if self.config.is_none() {
            return Err(not_configured());
        }
        if !state.signed_in || state.reauthorize {
            return Err(Failure::new(
                ErrorCode::SpotifyAuthRequired,
                "Sign in to Spotify before syncing.",
            ));
        }
        if state.authorizing {
            return Err(busy());
        }
        let retry_at = state.sync.error.as_ref().and_then(|error| error.retry_at);
        let error = state
            .sync
            .error
            .clone()
            .filter(|_| retry_at.is_some_and(|at| at > unix_now()));
        state.sync = SyncDto {
            running: true,
            error,
            ..SyncDto::default()
        };
        let accepted = state.sync.clone();
        let this = Arc::clone(self);
        if thread::Builder::new()
            .name("fsonos-spotify-sync".into())
            .spawn(move || this.run_sync(&store, retry_at))
            .is_err()
        {
            state.sync.running = false;
            state.sync.error = Some(SyncErrorDto {
                detail: "Could not start Spotify sync.".into(),
                retryable: true,
                retry_at: None,
            });
            return Err(internal());
        }
        Ok(accepted)
    }

    fn run_sync(&self, store: &SharedStore, retry_at: Option<i64>) {
        let result = self.run_session(async |session, cx| {
            let remaining = retry_at.unwrap_or_default().saturating_sub(unix_now());
            if let Ok(secs) = u64::try_from(remaining) {
                sleep(wall_now(), Duration::from_secs(secs)).await;
            }
            self.read_into_store(session, cx, store).await
        });
        if let Ok(mut state) = self.state.lock() {
            state.sync.running = false;
            if let Err(err) = result {
                state.reauthorize = matches!(&err, SpotifyError::Auth(text) if text.contains("invalid_grant"))
                    || matches!(&err, SpotifyError::Api { status: 401, .. });
                let retry_at = if matches!(err, SpotifyError::Api { status: 429, .. }) {
                    state.sync.error.as_ref().and_then(|e| e.retry_at)
                } else {
                    None
                };
                state.sync.error = Some(SyncErrorDto {
                    detail: safe_error(&err, self.cache.path()),
                    retryable: matches!(
                        err,
                        SpotifyError::Http(_)
                            | SpotifyError::Api {
                                status: 429 | 500..=599,
                                ..
                            }
                    ),
                    retry_at,
                });
            }
        }
    }
}

fn take_authorization(
    pending: &mut Option<TimedAuthorization>,
    now: Instant,
) -> Result<PendingAuthorization, &'static str> {
    let flow = pending
        .take()
        .ok_or("No pending Spotify sign-in. Start sign-in again.")?;
    if now.duration_since(flow.at) >= Duration::from_secs(600) {
        return Err("Spotify sign-in expired after ten minutes. Start sign-in again.");
    }
    Ok(flow.pending)
}

pub(crate) fn library_status(cache: &SpotifyCache) -> LibraryDto {
    LibraryDto {
        albums: cache.albums.iter().filter(|a| a.saved).count(),
        tracks: cache.liked_uris.len(),
        synced_at: cache.synced_at,
    }
}

pub(crate) fn not_configured() -> Failure {
    Failure::new(ErrorCode::SpotifyNotConfigured, "Spotify sign-in is not configured.")
        .with_hint("Set FSONOS_SPOTIFY_CLIENT_ID and register FSONOS_SPOTIFY_REDIRECT_URI in the Spotify dashboard.")
}
fn busy() -> Failure {
    Failure::new(
        ErrorCode::NotReady,
        "Spotify is busy; retry after sync or sign-in finishes.",
    )
    .with_hint("Wait for Spotify sign-in or sync to finish, then retry.")
}
fn internal() -> Failure {
    Failure::new(ErrorCode::Internal, "Spotify worker unavailable.")
}
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn safe_error(err: &SpotifyError, path: &Path) -> String {
    match err {
        SpotifyError::Auth(text) => {
            for code in [
                "invalid_grant",
                "invalid_client",
                "invalid_scope",
                "access_denied",
                "temporarily_unavailable",
                "server_error",
            ] {
                if text.contains(code) {
                    return format!("Spotify: {code}");
                }
            }
            "Spotify authorization failed. Start sign-in again.".into()
        }
        SpotifyError::Io(err) => format!(
            "Cannot read or write Spotify token cache {}: {}",
            path.display(),
            err.kind()
        ),
        SpotifyError::Api { status, .. } => format!("Spotify API returned HTTP {status}."),
        SpotifyError::Http(_) => "Spotify request failed. Check the connection and retry.".into(),
        SpotifyError::Decode(_) => "Spotify returned an unreadable response.".into(),
        SpotifyError::Store(_) | SpotifyError::Config(_) | SpotifyError::UnknownMood { .. } => {
            "Could not sync the Spotify library cache.".into()
        }
    }
}

fn html(status: u16, message: &str) -> Response {
    let message = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    Response::with_status(StatusCode::from_u16(status)).header("content-type", b"text/html; charset=utf-8".to_vec())
        .header("cache-control", b"no-store".to_vec())
        .body(ResponseBody::Bytes(format!("<!doctype html><html><head><title>Spotify</title></head><body><p>{message}</p></body></html>").into_bytes()))
}

impl crate::Surface {
    fn spotify_admit(
        &self,
        client: &fsonos_core::policy::Client,
        operation: &str,
        read: bool,
        local: bool,
    ) -> Result<(), Failure> {
        if local && *client != fsonos_core::policy::Client::LoopbackHttp {
            return Err(Failure::new(
                ErrorCode::ForbiddenNotLoopback,
                "Spotify sign-in requires a loopback caller.",
            ));
        }
        self.guard(client).authorize(operation, read)
    }

    pub(crate) fn spotify_login(
        &self,
        client: &fsonos_core::policy::Client,
    ) -> Result<String, Failure> {
        self.spotify_admit(client, "spotify_login", false, true)?;
        self.spotify.as_ref().ok_or_else(not_configured)?.login()
    }

    pub(crate) fn spotify_callback(
        &self,
        client: &fsonos_core::policy::Client,
        target: String,
    ) -> Response {
        if let Err(err) = self.spotify_admit(client, "spotify_callback", false, true) {
            return err.http_response();
        }
        match &self.spotify {
            Some(spotify) => spotify.callback(target),
            None => not_configured().http_response(),
        }
    }

    pub(crate) async fn spotify_exchange(
        &self,
        cx: &Cx,
        client: &fsonos_core::policy::Client,
        input: ExchangeRequest,
    ) -> Response {
        if let Err(err) = self.spotify_admit(client, "spotify_exchange", false, false) {
            return err.http_response();
        }
        let Some(spotify) = &self.spotify else {
            return not_configured().http_response();
        };
        let Some(store) = self.spotify_store() else {
            return internal().http_response();
        };
        spotify.exchange(cx, input, store).await
    }

    pub(crate) fn spotify_status(
        &self,
        client: &fsonos_core::policy::Client,
    ) -> Result<StatusDto, Failure> {
        self.spotify_admit(client, "spotify_status", true, false)?;
        let cache = self.with_store(|s| s.spotify_cache())?.unwrap_or_default();
        match &self.spotify {
            Some(spotify) => spotify.status(&cache),
            None => Ok(StatusDto {
                configured: false,
                client_id: None,
                app_redirect_uri: DEFAULT_APP_REDIRECT.into(),
                signed_in: false,
                reauthorize: false,
                library: library_status(&cache),
                sync: SyncDto::default(),
            }),
        }
    }

    pub(crate) fn spotify_sync(
        &self,
        client: &fsonos_core::policy::Client,
    ) -> Result<SyncDto, Failure> {
        self.spotify_admit(client, "spotify_sync", false, false)?;
        let spotify = self.spotify.as_ref().ok_or_else(not_configured)?;
        let store = self.spotify_store().ok_or_else(|| {
            Failure::new(ErrorCode::NotReady, "Spotify sync needs the daemon store.")
        })?;
        spotify.sync(store)
    }

    pub(crate) fn spotify_albums(
        &self,
        client: &fsonos_core::policy::Client,
        offset: usize,
        limit: usize,
        q: &str,
    ) -> Result<AlbumsDto, Failure> {
        self.spotify_admit(client, "list_spotify_albums", true, false)?;
        let cache = self.with_store(|s| s.spotify_cache())?.unwrap_or_default();
        let q = q.to_lowercase();
        let albums: Vec<_> = cache
            .albums
            .into_iter()
            .filter(|album| {
                album.saved
                    && format!("{} {}", album.title, album.artist)
                        .to_lowercase()
                        .contains(&q)
            })
            .collect();
        Ok(AlbumsDto {
            total: albums.len(),
            items: albums
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(AlbumDto::from)
                .collect(),
        })
    }

    pub(crate) fn spotify_album_tracks(
        &self,
        client: &fsonos_core::policy::Client,
        id: &str,
    ) -> Result<Vec<TrackDto>, Failure> {
        self.spotify_admit(client, "list_spotify_album_tracks", true, false)?;
        let uri = format!("spotify:album:{id}");
        let rows = self
            .with_store(|s| {
                let cache = s.spotify_cache()?;
                if !cache.albums.iter().any(|a| a.saved && a.uri == uri) {
                    return Ok(Vec::new());
                }
                let active: std::collections::HashSet<_> = cache.track_uris.into_iter().collect();
                Ok(s.library()?
                    .into_iter()
                    .filter(|row| active.contains(&row.track.source_uri))
                    .collect::<Vec<_>>())
            })?
            .unwrap_or_default();
        let mut tracks: Vec<_> = rows
            .iter()
            .filter(|row| row.album_uri.as_deref() == Some(uri.as_str()))
            .map(TrackDto::from)
            .collect();
        tracks.sort_by(|a, b| (a.disc, a.number, &a.uri).cmp(&(b.disc, b.number, &b.uri)));
        Ok(tracks)
    }

    pub(crate) fn spotify_tracks(
        &self,
        client: &fsonos_core::policy::Client,
        offset: usize,
        limit: usize,
        q: &str,
    ) -> Result<TracksDto, Failure> {
        self.spotify_admit(client, "list_spotify_tracks", true, false)?;
        let (cache, rows) = self
            .with_store(|s| Ok((s.spotify_cache()?, s.library()?)))?
            .unwrap_or_default();
        let q = q.to_lowercase();
        let liked: std::collections::HashSet<_> =
            cache.liked_uris.iter().map(String::as_str).collect();
        let tracks: Vec<_> = rows
            .iter()
            .filter(|row| {
                liked.contains(row.track.source_uri.as_str())
                    && format!(
                        "{} {} {}",
                        row.track.title,
                        row.track.artist.as_deref().unwrap_or_default(),
                        row.track.album.as_deref().unwrap_or_default()
                    )
                    .to_lowercase()
                    .contains(&q)
            })
            .collect();
        Ok(TracksDto {
            total: tracks.len(),
            items: tracks
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|row| {
                    let track = TrackDto::from(row);
                    LikedTrackDto {
                        id: track.id,
                        title: track.title,
                        artists: track.artists,
                        uri: track.uri,
                        duration_secs: track.duration_secs,
                        disc: track.disc,
                        number: track.number,
                        album: row.track.album.clone(),
                        art_url: cache
                            .albums
                            .iter()
                            .find(|a| Some(&a.uri) == row.album_uri.as_ref())
                            .and_then(|a| a.art_url.clone()),
                    }
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_verifier_and_code_boundaries_are_validated_without_echoing_input() {
        for verifier in ["a".repeat(43), "b".repeat(128), "-._~".repeat(16)] {
            for code in ["x".into(), "x".repeat(512)] {
                let req = ExchangeRequest {
                    code,
                    code_verifier: verifier.clone(),
                    redirect_uri: DEFAULT_APP_REDIRECT.into(),
                };
                assert!(validate_exchange(&req, DEFAULT_APP_REDIRECT).is_ok());
            }
        }
        for verifier in [
            "a".repeat(42),
            "a".repeat(129),
            "é".repeat(43),
            format!("{}!", "a".repeat(43)),
        ] {
            let req = ExchangeRequest {
                code: "private-code".into(),
                code_verifier: verifier.clone(),
                redirect_uri: DEFAULT_APP_REDIRECT.into(),
            };
            let err = validate_exchange(&req, DEFAULT_APP_REDIRECT).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidArgument);
            assert!(!err.detail.contains(&verifier));
            assert!(!err.detail.contains("private-code"));
        }
        for code in [String::new(), "x".repeat(513)] {
            let req = ExchangeRequest {
                code,
                code_verifier: "a".repeat(43),
                redirect_uri: DEFAULT_APP_REDIRECT.into(),
            };
            assert_eq!(
                validate_exchange(&req, DEFAULT_APP_REDIRECT)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidArgument
            );
        }
    }

    #[test]
    fn status_reports_configured_app_redirect_and_client_id() {
        let data_dir = fsonos_spotify::fake_spotify::scratch_dir("status-pure");
        for configured in [false, true] {
            let spotify = Spotify::new(
                configured.then(fsonos_spotify::fake_spotify::config),
                &data_dir,
                Endpoints::default(),
                None,
                "frankensonos://custom-callback".into(),
            )
            .unwrap();
            let status =
                serde_json::to_value(spotify.status(&SpotifyCache::default()).unwrap()).unwrap();
            assert_eq!(status["app_redirect_uri"], "frankensonos://custom-callback");
            if configured {
                assert_eq!(status["client_id"], fsonos_spotify::fake_spotify::CLIENT_ID);
            } else {
                assert!(status.get("client_id").is_none());
            }
            assert_eq!(status["signed_in"], false);
        }
    }

    #[test]
    fn authorization_expires_after_ten_minutes_and_is_consumed() {
        let config = SpotifyConfig {
            client_id: "0123456789abcdef".into(),
            redirect_uri: "http://127.0.0.1:8099/auth/spotify/callback".into(),
        };
        let now = Instant::now();
        let flow = || TimedAuthorization {
            pending: PendingAuthorization::begin(&config).unwrap(),
            at: now,
        };
        let mut pending = Some(flow());
        assert!(take_authorization(&mut pending, now + Duration::from_secs(599)).is_ok());
        assert!(take_authorization(&mut pending, now).is_err());
        let mut expired = Some(flow());
        assert!(take_authorization(&mut expired, now + Duration::from_secs(600)).is_err());
        assert!(expired.is_none());
    }

    #[test]
    fn error_pages_escape_markup_and_never_repeat_upstream_tokens() {
        let err =
            SpotifyError::Auth("token endpoint 400: invalid_grant (access-1 refresh-1)".into());
        assert_eq!(
            safe_error(&err, Path::new("/tmp/cache")),
            "Spotify: invalid_grant"
        );
        let html = html(400, "<script>bad</script>&\"");
        let (_, _, body) = html.into_parts();
        let ResponseBody::Bytes(bytes) = body else {
            panic!("HTML bytes")
        };
        let body = String::from_utf8(bytes).unwrap();
        assert!(body.contains("&lt;script&gt;"));
        assert!(!body.contains("<script>"));
    }
}
