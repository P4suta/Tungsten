use crate::{Error, sealed::RuntimeManifest, store::EncryptedDirectory};
use axum::{
    Extension, Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use zeroize::{Zeroize, Zeroizing};

struct DeviceSession {
    secret: Zeroizing<[u8; 32]>,
    vault: String,
    secure: bool,
}

impl DeviceSession {
    fn new(vault: String, secure: bool) -> Self {
        let mut secret = Zeroizing::new([0; 32]);
        secret[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        secret[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        Self {
            secret,
            vault,
            secure,
        }
    }

    fn mac(&self) -> Result<Hmac<Sha256>, Error> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&*self.secret).map_err(|_| Error::Worker)?;
        mac.update(b"tungsten.device-session.1");
        Ok(mac)
    }

    fn name(&self) -> String {
        format!(
            "{}tungsten-{}",
            if self.secure { "__Host-" } else { "" },
            self.vault
        )
    }

    fn permits(&self, headers: &HeaderMap) -> bool {
        let name = self.name();
        let mut token = None;
        for cookie in headers.get_all(header::COOKIE) {
            let Ok(cookie) = cookie.to_str() else {
                return false;
            };
            for pair in cookie.split(';') {
                if let Some((key, value)) = pair.trim().split_once('=')
                    && key == name
                {
                    if token.is_some() {
                        return false;
                    }
                    token = Some(value);
                }
            }
        }
        token
            .filter(|value| value.len() == 43)
            .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
            .is_some_and(|tag| self.mac().is_ok_and(|mac| mac.verify_slice(&tag).is_ok()))
    }

    fn cookie(&self) -> Result<String, Error> {
        let token = URL_SAFE_NO_PAD.encode(self.mac()?.finalize().into_bytes());
        Ok(format!(
            "{}={token}; Path=/; HttpOnly; SameSite=Strict{}",
            self.name(),
            if self.secure { "; Secure" } else { "" }
        ))
    }
}

struct Unlocked {
    router: Option<Router>,
    session: DeviceSession,
    failed: bool,
}

struct Server {
    directory: EncryptedDirectory,
    manifest: Option<RuntimeManifest>,
    unlocked: Option<Unlocked>,
    nonce: String,
    secure: bool,
    source: Option<PathBuf>,
    stopped: Arc<crate::store::MigrationStop>,
}

struct StopOnDrop(Arc<crate::store::MigrationStop>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

type Shared = Arc<Mutex<Server>>;

pub(super) fn app(
    path: &Path,
    origin: super::ServerOrigin,
    source: Option<PathBuf>,
) -> Result<Router, Error> {
    let directory = EncryptedDirectory::open(path)?;
    let manifest = directory.manifest()?;
    if source.is_none()
        && manifest
            .as_ref()
            .is_some_and(|manifest| manifest.phase == crate::sealed::VaultPhase::Migrating)
    {
        return Err(Error::InvalidData(
            "Resume the encrypted copy with --migrate-from and the original source directory.",
        ));
    }
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let stopped = Arc::new(crate::store::MigrationStop::default());
    let stop = Arc::new(StopOnDrop(Arc::clone(&stopped)));
    let state = Arc::new(Mutex::new(Server {
        directory,
        manifest,
        unlocked: None,
        nonce: nonce.clone(),
        secure: origin.secure(),
        source,
        stopped,
    }));
    Ok(Router::new()
        .route("/", get(super::index))
        .route("/trash", get(super::index))
        .route("/healthz", get(|| async { StatusCode::NO_CONTENT }))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/assets/{*path}", get(super::browser_asset))
        .route("/vendor/pdf/{*path}", get(super::vendor::pdf))
        .route("/api/vault", get(status).post(initialize))
        .route("/api/vault/unlock", post(unlock))
        .fallback(private)
        .layer(DefaultBodyLimit::max(4096))
        .layer(Extension(state))
        .layer(Extension(stop))
        .layer(middleware::from_fn(super::boundary))
        .layer(Extension(origin))
        .layer(Extension(super::SealedBrowser))
        .layer(Extension(nonce)))
}

#[derive(Serialize)]
struct Status {
    manifest: Option<RuntimeManifest>,
    unlocked: bool,
    opening: bool,
    failed: bool,
}

async fn status(Extension(state): Extension<Shared>, headers: HeaderMap) -> Json<Status> {
    let state = state.lock().await;
    let opened = state
        .unlocked
        .as_ref()
        .filter(|opened| opened.session.permits(&headers));
    Json(Status {
        manifest: state.manifest.clone(),
        unlocked: opened.is_some_and(|opened| opened.router.is_some()),
        opening: opened.is_some_and(|opened| opened.router.is_none() && !opened.failed),
        failed: opened.is_some_and(|opened| opened.failed),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Unlock {
    seed: String,
}

impl Drop for Unlock {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Initialize {
    manifest: RuntimeManifest,
    #[serde(flatten)]
    unlock: Unlock,
}

async fn initialize(
    Extension(state): Extension<Shared>,
    Json(input): Json<Initialize>,
) -> Result<Response, Error> {
    open(state, input.unlock, Some(input.manifest)).await
}

async fn unlock(
    Extension(state): Extension<Shared>,
    Json(input): Json<Unlock>,
) -> Result<Response, Error> {
    open(state, input, None).await
}

async fn open(
    state: Shared,
    input: Unlock,
    manifest: Option<RuntimeManifest>,
) -> Result<Response, Error> {
    if input.seed.len() != 43 {
        return Err(Error::VaultKey);
    }
    let mut seed = Zeroizing::new([0; 32]);
    if URL_SAFE_NO_PAD
        .decode_slice(&input.seed, &mut *seed)
        .map_err(|_| Error::VaultKey)?
        != 32
    {
        return Err(Error::VaultKey);
    }
    let shared = Arc::clone(&state);
    let mut state = state.lock_owned().await;
    tokio::task::spawn_blocking(move || {
        state.manifest = state.directory.manifest()?;
        let selected = match (&state.manifest, manifest) {
            (Some(current), Some(requested)) if current.vault != requested.vault => {
                return Err(Error::Exists);
            }
            (Some(current), _) => current.clone(),
            (None, Some(requested)) if requested.phase == crate::sealed::VaultPhase::Pending => {
                requested
            }
            (None, Some(_)) => {
                return Err(Error::InvalidData(
                    "Initialize an empty vault before opening it.",
                ));
            }
            (None, None) => return Err(Error::VaultKey),
        };
        let key = selected.database_key(&seed)?;
        let selected =
            if state.source.is_some() && selected.phase != crate::sealed::VaultPhase::Ready {
                selected.migrating(&seed)?
            } else {
                selected
            };
        if state.unlocked.as_ref().is_none_or(|opened| opened.failed) {
            state.directory.save_manifest(&selected)?;
            state.manifest = Some(selected.clone());
            if selected.phase != crate::sealed::VaultPhase::Ready
                && let Some(source) = state.source.clone()
            {
                let directory = state.directory.duplicate()?;
                state.unlocked = Some(Unlocked {
                    router: None,
                    session: DeviceSession::new(selected.vault.id.clone(), state.secure),
                    failed: false,
                });
                let nonce = state.nonce.clone();
                let stopped = Arc::clone(&state.stopped);
                tokio::runtime::Handle::current().spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        let store = directory.migrate(&source, &key, stopped, |_| Ok(()))?;
                        let ready = selected.ready(&seed)?;
                        directory.save_manifest(&ready)?;
                        Ok::<_, Error>((store, ready))
                    })
                    .await
                    .map_err(|_| Error::Worker)
                    .and_then(|result| result);
                    let mut state = shared.lock().await;
                    let result = result.and_then(|(store, ready)| {
                        let router = super::with_store(store, nonce)?;
                        state.manifest = Some(ready);
                        Ok(router)
                    });
                    if let Some(opened) = &mut state.unlocked {
                        match result {
                            Ok(router) => opened.router = Some(router),
                            Err(_) => {
                                opened.failed = true;
                                eprintln!("operation=migration outcome=failed");
                            }
                        }
                    }
                });
            } else {
                let store = state.directory.store(&key)?;
                let ready = selected.ready(&seed)?;
                state.directory.save_manifest(&ready)?;
                state.manifest = Some(ready);
                state.unlocked = Some(Unlocked {
                    router: Some(super::with_store(store, state.nonce.clone())?),
                    session: DeviceSession::new(selected.vault.id, state.secure),
                    failed: false,
                });
            }
        }
        let cookie = state
            .unlocked
            .as_ref()
            .ok_or(Error::Worker)?
            .session
            .cookie()?;
        let status = if state
            .unlocked
            .as_ref()
            .is_some_and(|opened| opened.router.is_some())
        {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::ACCEPTED
        };
        Ok((status, [(header::SET_COOKIE, cookie)]).into_response())
    })
    .await
    .map_err(|_| Error::Worker)?
}

async fn private(Extension(state): Extension<Shared>, request: axum::extract::Request) -> Response {
    let state = state.lock().await;
    let router = state
        .unlocked
        .as_ref()
        .filter(|opened| {
            tungsten_state::private_access(
                state.manifest.as_ref().map(|manifest| manifest.phase),
                opened.session.permits(request.headers()),
                opened.router.is_some(),
                opened.failed,
            )
        })
        .and_then(|opened| opened.router.clone());
    drop(state);
    let Some(router) = router else {
        if request.method() == axum::http::Method::GET
            && let Some(id) = request.uri().path().strip_prefix("/.attachments/")
            && crate::rules::valid_attachment(id.as_bytes())
        {
            return Redirect::to(&format!("/?file={id}")).into_response();
        }
        return Error::VaultKey.into_response();
    };
    match router.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}
