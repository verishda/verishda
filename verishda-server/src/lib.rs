use std::cell::OnceCell;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use axum::body::Body;
use axum::debug_handler;
use axum::extract::{ws, FromRef, Host, OriginalUri, Query, State};
use axum::{Router, routing::{get, post, put}, response::{Response, IntoResponse, Redirect, Html}, Json, extract::{Path, FromRequestParts}, async_trait, RequestPartsExt, Extension};
use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum_extra::{TypedHeader, headers::{Authorization, authorization::Bearer}};
use axum_extra::typed_header::TypedHeaderRejectionReason;
use bytes::Bytes;
use verishda_config::Config;
use dashmap::DashMap;
use error::HandlerError;
use http::{StatusCode, request::Parts};
use memory_store::MemoryStore;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use verishda_dto::types::{PresenceAnnouncement, Site, Presence};
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
mod scheme;
mod datamodel;

refinery::embed_migrations!("migrations");

const SWAGGER_SPEC_URL: &str = "/api/public/openapi.yaml";

struct VerishdaState
where Self: Send + Sync + Clone
{
    pool: Pool<Postgres>,
    config: Box<dyn Config>,
    pending_logins: Arc<DashMap<String,oneshot::Sender<String>>>,
}
impl Clone for VerishdaState {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            config: self.config.clone_box_dyn(),
            pending_logins: self.pending_logins.clone(),
        }
    }
}

pub fn init_logging(cfg: impl verishda_config::Config) {
    let rust_log_config = cfg.get("RUST_LOG").ok();
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
impl FromRef<VerishdaState> for ConnectionPool {
    fn from_ref(state: &VerishdaState) -> Self {
        state.pool.clone()
    }
}

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

async fn migrate_db(url: &str) -> Result<()> {
    // create connection for db migration to use
    let mut config = tokio_postgres::Config::from_str(url)?;
    let executable = std::env::args().into_iter().next().unwrap();
    config.application_name(&executable);
    let (mut client, con) = config.connect(tokio_postgres::NoTls).await?;

    // spawn connection handler in background
    tokio::spawn(async move {
        if let Err(e) = con.await {
            log::error!("connection error while database migration: {}", e);
        }
    });

    // run migrations
    log::info!("checking database for potential migrations...");
    let report = migrations::runner().run_async(&mut client).await?;

    // log migration results
    if report.applied_migrations().is_empty() {
        log::info!("database is up to date, no migrations applied.")
    } else {
        log::info!("applied migrations:");
        for m in report.applied_migrations() {
            log::info!("\t{m}");
        }
    }

    Ok(())
}


pub async fn connect_db(url: &str) -> Result<Pool<Postgres>> {
    migrate_db(url).await?;

    // provide connection pool
    Ok(Pool::connect(&url).await?)
}

pub fn build_router(pool: Pool<Postgres>, config: impl verishda_config::Config) -> Router
{
    let pending_logins = Arc::new(DashMap::with_capacity(127));
    let state = VerishdaState { pool, config: config.clone_box_dyn(), pending_logins };
    return Router::new()
    .route(SWAGGER_SPEC_URL, get(handle_get_swagger_spec))
    .route("/api/public/swagger-ui/:path", get(handle_get_swagger_ui))
    .route("/api/public/oidc/login-requests/:login_id", get(handle_get_login_request))
    .route("/api/public/oidc/login-target", get(handle_get_login_target))
    .route("/api/sites", get(handle_get_sites))
    .route("/api/sites/:siteId/presence", get(handle_get_sites_siteid_presence))
    .route("/api/sites/:siteId/hello", post(handle_post_sites_siteid_hello))
    .route("/api/sites/:siteId/announce", put(handle_put_announce))
    .route("/", get(handle_get_fallback))
    .route("/*path", get(handle_get_fallback))
    .layer(Extension(MemoryStore::new()))
    .with_state(state)

}

#[debug_handler(state=VerishdaState)]
async fn handle_get_fallback(Scheme(scheme): Scheme, Host(host): Host, OriginalUri(path): OriginalUri) -> Result<Redirect, HandlerError> {
    let full_url = format!("{scheme}://{host}{path}");
    trace!("full_url: {full_url}");

    let mut redirect_url = http::Uri::try_from(full_url)?.into_parts();
    redirect_url.path_and_query = Some(http::uri::PathAndQuery::from_static("/api/public/swagger-ui/oauth2-redirect.html"));
    let redirect_url = http::Uri::from_parts(redirect_url)?.to_string();
    
    let swagger_ui_url = format!("/api/public/swagger-ui/index.html?url={SWAGGER_SPEC_URL}&oauth2RedirectUrl={redirect_url}");
    Ok(Redirect::temporary(&swagger_ui_url))
}

async fn handle_get_swagger_spec() -> Result<Response<Body>, HandlerError> {
    let resp = Response::builder()
    .status(200)
    .body(Body::from(Bytes::copy_from_slice(SWAGGER_SPEC.get_or_init(||swagger_ui::swagger_spec_file!("../../verishda.yaml")).content)))
    ?;

    Ok(resp)
}

#[debug_handler]
async fn handle_get_swagger_ui(Path(path): Path<String>) -> Result<Response<Body>, HandlerError>{

    let resp = match swagger_ui::Assets::get(&path) {
        Some(data) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            http::Response::builder()
            .status(200)
            .header("Content-Type", mime.to_string())
            .body(Body::from(bytes::Bytes::copy_from_slice(data.as_ref())))?
        },
        None => http::Response::builder()
            .status(404)
            .body(Body::from(bytes::Bytes::from("404 Not Found".as_bytes())))?
    };

    Ok(resp)
}

#[derive(Serialize,Deserialize)]
struct CodeAndStateParams {
    code: String,
    state: String,
}




#[debug_handler]
async fn handle_get_login_request(State(state): State<VerishdaState>, Path(login_id): Path<String>, ws: WebSocketUpgrade) -> impl IntoResponse {

    let (tx, rx) = oneshot::channel::<String>();

    let prev = state.pending_logins.insert(login_id, tx);
    if let Some(_) = prev {
        return Err(Response::builder().status(409).body("login request already exists, terminating both".to_string()).unwrap());
    };

    Ok(ws.on_upgrade(|socket|handle_login_request_ws(socket, rx)))
}

async fn handle_login_request_ws(mut socket: WebSocket, pending_login: oneshot::Receiver<String>) {
    let code = match pending_login.await {
        Ok(code) => code,
        Err(e) => {
            // we simply return on Err, there does not seem to be a way to distinguish between
            // a closed oneshot and other errors
            log::debug!("oneshot ended without receiving code: {e}");
            return;
        }
    };

    // we ignore the result, as there is no distinction between a closed websocket (fine, and )    
    if let Err(e) = socket.send(ws::Message::from(code)).await {
        log::debug!("failed to send code to web socket: {e}")
    }
}

#[debug_handler]
async fn handle_get_login_target(State(state): State<VerishdaState>, Query(code_state): Query<CodeAndStateParams>) -> Result<(), Response<String>> {
    match state.pending_logins.remove(&code_state.state) {
        Some((_,tx)) => {
            let code = code_state.code.clone();
            if let Err(e) = tx.send(code) {
                return Err(Response::builder().status(404).body("login terminated before code could be sent".to_string()).unwrap())
            }
            Ok(())
        },
        None => {
            Err(Response::builder().status(404).body("no pending login with this id".to_string()).unwrap())
        }

    }
}

fn range_from(offset: Option<u32>, limit: Option<u32>) -> std::ops::Range<u32> {
    let start = if let Some(offset) = offset { offset } else {0};
    let end = if let Some(limit) = limit {start + limit} else {u32::MAX};
    std::ops::Range {start, end}
}

#[derive(Deserialize)]
struct PresenceQueryParams {
    term: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>
}

#[debug_handler]
async fn handle_get_sites_siteid_presence(DbCon(mut con): DbCon, _: State<VerishdaState>, auth_info: AuthInfo, Path(site_id): Path<String>, Query(query): Query<PresenceQueryParams>) -> Result<Json<Vec<Presence>>, HandlerError> 
{   
    let term = query.term.as_ref().map(|s|s.as_str());
    let range = range_from(query.offset, query.limit);
    let presences = site::get_presence_on_site(&mut con, &auth_info.subject, &to_logged_as_name(&auth_info), &site_id, range, term).await?;
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

#[debug_handler(state=VerishdaState)]
async fn handle_post_sites_siteid_hello(mut dbcon: DbCon, s: State<VerishdaState>, auth_info: AuthInfo, Path(site_id): Path<String>, _: State<ConnectionPool>) -> Result<(), HandlerError> {

    let logged_as_name = to_logged_as_name(&auth_info);
    site::hello_site(&mut dbcon.0, &auth_info.subject, &logged_as_name, &site_id).await?;
    Ok(())
}

#[debug_handler]
async fn handle_put_announce(DbCon(mut con): DbCon, _: State<VerishdaState>, auth_info: AuthInfo, Path(site_id): Path<String>, Json(announcements): Json<Vec<PresenceAnnouncement>>) -> Result<impl IntoResponse, HandlerError> {

    site::announce_presence_on_site(&mut con, &auth_info.subject, &site_id, &to_logged_as_name(&auth_info), &announcements).await?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())?
    )
}

#[debug_handler]
async fn handle_get_sites(DbCon(mut con): DbCon, State(_state): State<VerishdaState>, _auth_info: AuthInfo) -> Result<Json<Vec<Site>>, HandlerError> {
    let sites = site::get_sites(&mut con).await?;
    Ok(Json(sites))
}

#[async_trait]
impl FromRequestParts<VerishdaState> for AuthInfo
where
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &VerishdaState) -> Result<Self, Self::Rejection> {
        trace!("checking authorization...");
        
        
        let mut ox = oidc::OidcExtension::default();
        let issuer_url = state.config.get("ISSUER_URL").or(Err(AuthError::ConfigurationError(anyhow!("ISSUER_URL not defined. Use a URL that can serve as a base URL for OIDC discovery"))))?;
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


