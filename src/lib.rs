use std::cell::OnceCell;
use std::ops::{Deref, DerefMut};

use anyhow::{anyhow, Result};
use axum::body::{Body, HttpBody};
use axum::debug_handler;
use axum::extract::{OriginalUri, Host, FromRef, State};
use axum::{Router, routing::{get, post, put}, response::{Response, IntoResponse, Redirect, Html}, Json, body::{Full, Empty}, extract::{Path, FromRequestParts, rejection::TypedHeaderRejectionReason}, async_trait, TypedHeader, headers::{Authorization, authorization::Bearer}, RequestPartsExt, Extension};

use bytes::Bytes;
use error::HandlerError;
use http::{StatusCode, request::Parts};
use memory_store::MemoryStore;
use dotenv::dotenv;

use site::{PresenceAnnouncement, Site, Presence};
use log::{trace, error};
use sqlx::pool::PoolConnection;
use sqlx::{Pool, Postgres};

use crate::oidc_cache::MetadataCache;
use crate::scheme::Scheme;


const SWAGGER_SPEC: OnceCell<swagger_ui::Spec> = OnceCell::new();

#[derive(Debug)]
struct AuthInfo {
    subject: String,
    given_name: Option<String>,
    family_name: Option<String>,
}

mod site;
mod oidc;
mod store;
mod memory_store;
mod oidc_cache;
mod error;
pub mod config;
mod scheme;

const SWAGGER_SPEC_URL: &str = "/api/public/openapi.yaml";

pub fn init_logging() {
    let rust_log_config = config::get("rust_log").ok();
    let mut logger_builder = env_logger::builder();
    if let Some(rust_log) = rust_log_config {
        logger_builder.parse_filters(&rust_log);
    } else {
        logger_builder.filter_level(log::LevelFilter::Info);
    }
    logger_builder.init();
    println!("max logging level is: {}.", log::max_level());
    println!("Use RUST_LOG environment variable to set one of the levels, e.g. RUST_LOG=error");
}

type ConnectionPool = Pool<Postgres>;
struct DbCon(PoolConnection<Postgres>);
impl Deref for DbCon {
    type Target = PoolConnection<Postgres>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DbCon {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for DbCon
where
    ConnectionPool: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pool = ConnectionPool::from_ref(state);

        let conn = pool.acquire().await.map_err(internal_error)?;

        Ok(Self(conn))
    }
}
fn internal_error<E>(err: E) -> (StatusCode, String)
where
    E: std::error::Error,
{
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

pub fn build_router(pool: Pool<Postgres>) -> Router
{
    return Router::new()
    .route(SWAGGER_SPEC_URL, get(handle_get_swagger_spec))
    .route("/api/public/swagger-ui/:path", get(handle_get_swagger_ui))
    .route("/api/sites", get(handle_get_sites))
    .route("/api/sites/:siteId/presence", get(handle_get_sites_siteid_presence))
    .route("/api/sites/:siteId/hello", post(handle_post_sites_siteid_hello))
    .route("/api/sites/:siteId/announce", put(handle_put_announce))
    .route("/", get(handle_get_fallback))
    .route("/*path", get(handle_get_fallback))
    .layer(Extension(MemoryStore::new()))
    .with_state(pool)

}

#[debug_handler]
async fn handle_get_fallback(Scheme(scheme): Scheme, Host(host): Host, OriginalUri(path): OriginalUri) -> Result<Redirect, HandlerError> {
    let full_url = format!("{scheme}://{host}{path}");
    trace!("full_url: {full_url}");

    let mut redirect_url = http::Uri::try_from(full_url)?.into_parts();
    redirect_url.path_and_query = Some(http::uri::PathAndQuery::from_static("/api/public/swagger-ui/oauth2-redirect.html"));
    let redirect_url = http::Uri::from_parts(redirect_url)?.to_string();
    
    let swagger_ui_url = format!("/api/public/swagger-ui/index.html?url={SWAGGER_SPEC_URL}&oauth2RedirectUrl={redirect_url}");
    Ok(Redirect::temporary(&swagger_ui_url))
}

async fn handle_get_swagger_spec() -> Result<Response<Full<Bytes>>, HandlerError> {
    let resp = Response::builder()
    .status(200)
    .body(Full::new(Bytes::copy_from_slice(SWAGGER_SPEC.get_or_init(||swagger_ui::swagger_spec_file!("./verishda.yaml")).content)))
    ?;

    Ok(resp)
}

#[debug_handler]
async fn handle_get_swagger_ui(Path(path): Path<String>) -> Result<Response<Full<Bytes>>, HandlerError>{

    let resp = match swagger_ui::Assets::get(&path) {
        Some(data) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            http::Response::builder()
            .status(200)
            .header("Content-Type", mime.to_string())
            .body(Full::new(bytes::Bytes::copy_from_slice(data.as_ref())))?
        },
        None => http::Response::builder()
            .status(404)
            .body(Full::new(bytes::Bytes::from("404 Not Found".as_bytes())))?
    };

    Ok(resp)
}

#[debug_handler]
async fn handle_get_sites_siteid_presence(DbCon(mut con): DbCon, _: State<ConnectionPool>, _auth_info: AuthInfo, Path(site_id): Path<String>) -> Result<Json<Vec<Presence>>, HandlerError> {
    let presences = site::get_presence_on_site(&mut con, &site_id).await?;
    Ok(Json(presences))
}


fn to_logged_as_name(auth_info: &AuthInfo) -> String {
    auth_info.given_name
    .iter()
    .chain(auth_info.family_name.iter())
    .fold(String::new(), |a,s| a + " " + s.as_str())
    .trim()
    .to_string()
}

#[debug_handler]
async fn handle_post_sites_siteid_hello(mut dbcon: DbCon, s: State<ConnectionPool>, auth_info: AuthInfo, Path(site_id): Path<String>, _: State<ConnectionPool>) -> Result<Json<Vec<Presence>>, HandlerError> {

    let logged_as_name = to_logged_as_name(&auth_info);
    site::hello_site(&mut dbcon.0, &auth_info.subject, &logged_as_name, &site_id).await?;

    // to return the current presences, proceed like with an ordinary
    // presence request
    return handle_get_sites_siteid_presence(dbcon, s, auth_info, Path(site_id)).await

}

#[debug_handler]
async fn handle_put_announce(DbCon(mut con): DbCon, _: State<ConnectionPool>, auth_info: AuthInfo, Path(site_id): Path<String>, Json(announcements): Json<Vec<PresenceAnnouncement>>) -> Result<impl IntoResponse, HandlerError> {

    site::announce_presence_on_site(&mut con, &auth_info.subject, &site_id, &to_logged_as_name(&auth_info), &announcements).await?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Empty::new())?
    )
}

#[debug_handler]
async fn handle_get_sites(DbCon(mut con): DbCon, State(_state): State<ConnectionPool>, _auth_info: AuthInfo) -> Result<Json<Vec<Site>>, HandlerError> {
    let sites = site::get_sites(&mut con).await?;
    Ok(Json(sites))
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthInfo
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        trace!("checking authorization...");
        
        
        let mut ox = oidc::OidcExtension::default();
        let issuer_url = config::get("issuer_url").or(Err(AuthError::ConfigurationError(anyhow!("issuer_url not defined. Use a URL that can serve as a base URL for OIDC discovery"))))?;
        let store = parts.extensions.get::<MemoryStore>().expect("memory store not set");
        let cache = MetadataCache::new(store.clone());
        if let Err(e) = ox.init(cache, &issuer_url).await {
            return Err(AuthError::ConfigurationError(e))
        }
            // Extract the token from the authorization header
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|e| {match e.reason() {
                &TypedHeaderRejectionReason::Missing => AuthError::TokenMissing,
                &_ => AuthError::InvalidToken,
            }})?;
        // Decode the user data
        let auth_info_opt = ox.check_auth_token(bearer.token());
        trace!("auth_info {auth_info_opt:?}");
        match auth_info_opt {
            Ok(auth_info) => Ok(auth_info),
            Err(e) => {
                error!("auth error: {e}");
                Err(AuthError::InvalidToken)
            }
        }
    
    }
}

fn status400(msg: &str) -> Response {
    status_html_of(StatusCode::BAD_REQUEST, msg)
}

enum AuthError {
    TokenMissing,
    TokenExpired,
    InvalidToken,
    ConfigurationError(anyhow::Error),
}

fn status_html_of(status: StatusCode, html_str: &str) -> Response {
    let mut resp = Html::from(html_str.to_string()).into_response();
    *resp.status_mut() = status;
    resp
}

impl IntoResponse for AuthError
{
    fn into_response(self) -> Response {
        match self {
            AuthError::ConfigurationError(error) => status_html_of(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("<h1>Authorization Configuration Error</h1><p>{error}</p>")
            ),
            _ => status_html_of(StatusCode::UNAUTHORIZED, "<h1>Unauthorized</h1>"),
        }
    }
}


