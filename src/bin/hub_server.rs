//! First-party NELRoot Image Hub HTTP service.
//!
//! Run with NELROOT_HUB_DATA_DIR, NELROOT_HUB_PUBLIC_URL and
//! NELROOT_HUB_PUBLISH_TOKEN configured. Public reads are unauthenticated;
//! artifact uploads and manifest publication require the bearer token.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::DefaultBodyLimit,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use nelroot::core::hub::{self, HubCatalog, HubCatalogEntry, HubManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{fs, net::TcpListener, sync::Mutex};

const MAX_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    root: PathBuf,
    public_url: String,
    publish_token: Arc<String>,
    catalog_lock: Arc<Mutex<()>>,
}

#[derive(Debug)]
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorResponse { error: self.1 })).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct UploadResponse {
    sha256: String,
    size_bytes: usize,
    stored: bool,
}

#[derive(Serialize)]
struct PublishResponse {
    id: String,
    manifest_sha256: String,
    manifest_url: String,
    catalog_entries: usize,
}

type ApiResult<T> = Result<T, ApiError>;

fn api_error(status: StatusCode, message: impl Into<String>) -> ApiError {
    ApiError(status, message.into())
}

fn digest_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn artifact_path(state: &AppState, digest: &str) -> PathBuf {
    state.root.join("artifacts").join(format!("{digest}.tar.gz"))
}

fn manifest_path(state: &AppState, digest: &str) -> PathBuf {
    state.root.join("manifests").join(format!("{digest}.json"))
}

fn catalog_path(state: &AppState) -> PathBuf {
    state.root.join("catalog.json")
}

fn authorized(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.publish_token.as_str())
}

fn require_auth(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if authorized(headers, state) {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::UNAUTHORIZED,
            "valid bearer token required",
        ))
    }
}

async fn write_atomic(path: &FsPath, bytes: &[u8]) -> ApiResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "invalid storage path"))?;
    fs::create_dir_all(parent)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| api_error(StatusCode::INTERNAL_SERVER_ERROR, "invalid storage filename"))?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    fs::write(&temporary, bytes)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if let Err(error) = fs::rename(&temporary, path).await {
        let _ = fs::remove_file(&temporary).await;
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not atomically store file: {error}"),
        ));
    }
    Ok(())
}

async fn load_catalog_file(state: &AppState) -> ApiResult<HubCatalog> {
    let path = catalog_path(state);
    let bytes = fs::read(&path)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let catalog: HubCatalog = serde_json::from_slice(&bytes)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    hub::validate_catalog(&catalog)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok(catalog)
}

async fn verify_artifact(state: &AppState, digest: &str) -> ApiResult<Vec<u8>> {
    if !valid_digest(digest) {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid SHA-256 identifier"));
    }
    let path = artifact_path(state, digest);
    let metadata = fs::metadata(&path)
        .await
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "artifact not found"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES as u64 {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "stored artifact has an invalid size or file type",
        ));
    }
    let bytes = fs::read(path)
        .await
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if digest_hex(&bytes) != digest {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "stored artifact failed SHA-256 verification",
        ));
    }
    Ok(bytes)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "NELRoot Image Hub",
        "status": "ok",
        "api_version": 1
    }))
}

async fn get_catalog(State(state): State<AppState>) -> ApiResult<Json<HubCatalog>> {
    Ok(Json(load_catalog_file(&state).await?))
}

async fn get_manifest(
    State(state): State<AppState>,
    Path(digest): Path<String>,
) -> ApiResult<Json<HubManifest>> {
    if !valid_digest(&digest) {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid manifest SHA-256"));
    }
    let path = manifest_path(&state, &digest);
    let bytes = fs::read(path)
        .await
        .map_err(|_| api_error(StatusCode::NOT_FOUND, "manifest not found"))?;
    if digest_hex(&bytes) != digest {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "stored manifest failed SHA-256 verification",
        ));
    }
    let manifest: HubManifest = serde_json::from_slice(&bytes)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    hub::validate_manifest(&manifest)
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(Json(manifest))
}

async fn get_artifact(
    State(state): State<AppState>,
    Path(digest): Path<String>,
) -> ApiResult<Response> {
    let bytes = verify_artifact(&state, &digest).await?;
    let mut response = Response::new(axum::body::Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/gzip"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

async fn upload_artifact(
    State(state): State<AppState>,
    Path(digest): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<UploadResponse>)> {
    require_auth(&headers, &state)?;
    if !valid_digest(&digest) {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid SHA-256 identifier"));
    }
    if body.is_empty() || body.len() > MAX_ARTIFACT_BYTES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "artifact must be between 1 byte and 512 MiB",
        ));
    }
    if digest_hex(&body) != digest {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "uploaded artifact does not match its SHA-256 identifier",
        ));
    }

    let path = artifact_path(&state, &digest);
    let stored = if path.exists() {
        let existing = verify_artifact(&state, &digest).await?;
        if existing.as_slice() != body.as_ref() {
            return Err(api_error(
                StatusCode::CONFLICT,
                "immutable artifact identifier already exists with different content",
            ));
        }
        false
    } else {
        write_atomic(&path, &body).await?;
        true
    };

    Ok((
        if stored { StatusCode::CREATED } else { StatusCode::OK },
        Json(UploadResponse {
            sha256: digest,
            size_bytes: body.len(),
            stored,
        }),
    ))
}

async fn publish_manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<PublishResponse>)> {
    require_auth(&headers, &state)?;
    if body.is_empty() || body.len() > MAX_MANIFEST_BYTES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "manifest must be between 1 byte and 8 MiB",
        ));
    }

    let mut manifest: HubManifest = serde_json::from_slice(&body)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    hub::validate_manifest(&manifest)
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, error))?;

    let _guard = state.catalog_lock.lock().await;
    let artifact_digest = manifest.image.rootfs.sha256.to_ascii_lowercase();
    let artifact = verify_artifact(&state, &artifact_digest).await?;
    if artifact.len() as u64 != manifest.image.rootfs.size_bytes {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "manifest rootfs size_bytes does not match the stored artifact",
        ));
    }

    manifest.image.rootfs.download_url =
        format!("{}/v1/artifacts/{artifact_digest}", state.public_url);
    hub::validate_manifest(&manifest)
        .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, error))?;

    let mut catalog = load_catalog_file(&state).await?;
    if catalog.images.iter().any(|entry| entry.id == manifest.image.id) {
        return Err(api_error(
            StatusCode::CONFLICT,
            "image ID already exists; published image IDs are immutable",
        ));
    }

    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let manifest_digest = digest_hex(&manifest_bytes);
    let manifest_url = format!("{}/v1/manifests/{manifest_digest}", state.public_url);
    let entry = HubCatalogEntry {
        id: manifest.image.id.clone(),
        name: manifest.image.name.clone(),
        version: manifest.image.version.clone(),
        distribution: manifest.image.distribution.clone(),
        architecture: manifest.image.architecture.clone(),
        description: manifest.image.description.clone(),
        manifest_url: manifest_url.clone(),
        manifest_sha256: manifest_digest.clone(),
        tags: manifest.image.tags.clone(),
    };

    write_atomic(&manifest_path(&state, &manifest_digest), &manifest_bytes).await?;
    catalog.images.push(entry);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .as_secs();
    catalog.generated_at = format!("unix:{now}");
    hub::validate_catalog(&catalog)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let catalog_bytes = serde_json::to_vec_pretty(&catalog)
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    write_atomic(&catalog_path(&state), &catalog_bytes).await?;

    Ok((
        StatusCode::CREATED,
        Json(PublishResponse {
            id: manifest.image.id,
            manifest_sha256: manifest_digest,
            manifest_url,
            catalog_entries: catalog.images.len(),
        }),
    ))
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name).map_err(|_| format!("required environment variable {name} is not set").into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        env::var_os("NELROOT_HUB_DATA_DIR").unwrap_or_else(|| "./nelroot-hub-data".into()),
    );
    let public_url = required_env("NELROOT_HUB_PUBLIC_URL")?
        .trim_end_matches('/')
        .to_string();
    let parsed_url = reqwest::Url::parse(&public_url)?;
    if parsed_url.scheme() != "https"
        || parsed_url.host_str().is_none()
        || !parsed_url.username().is_empty()
        || parsed_url.password().is_some()
    {
        return Err("NELROOT_HUB_PUBLIC_URL must be an HTTPS URL without credentials".into());
    }

    let publish_token = required_env("NELROOT_HUB_PUBLISH_TOKEN")?;
    if publish_token.len() < 32 {
        return Err("NELROOT_HUB_PUBLISH_TOKEN must contain at least 32 bytes".into());
    }
    let bind = env::var("NELROOT_HUB_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());

    fs::create_dir_all(root.join("artifacts")).await?;
    fs::create_dir_all(root.join("manifests")).await?;
    let catalog_file = root.join("catalog.json");
    if !catalog_file.exists() {
        let initial = HubCatalog {
            schema_version: 1,
            generated_at: "unix:0".into(),
            images: Vec::new(),
        };
        fs::write(&catalog_file, serde_json::to_vec_pretty(&initial)?).await?;
    }
    hub::load_catalog(&catalog_file)?;

    let state = AppState {
        root,
        public_url,
        publish_token: Arc::new(publish_token),
        catalog_lock: Arc::new(Mutex::new(())),
    };

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/catalog", get(get_catalog))
        .route("/v1/manifests/{digest}", get(get_manifest))
        .route(
            "/v1/artifacts/{digest}",
            get(get_artifact).post(upload_artifact),
        )
        .route("/v1/manifests", post(publish_manifest))
        .layer(DefaultBodyLimit::max(MAX_ARTIFACT_BYTES))
        .with_state(state);

    let listener = TcpListener::bind(&bind).await?;
    println!("NELRoot Image Hub listening on http://{bind}");
    println!("Public URL configured as {parsed_url}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
