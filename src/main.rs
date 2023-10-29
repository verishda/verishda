use std::cell::OnceCell;

use anyhow::{anyhow, Result};
use axum::debug_handler;
use axum::extract::OriginalUri;
use axum::{Router, routing::{get, post, put, any_service}, response::{Response, IntoResponse, Redirect, Html}, Json, body::{Full, Body, Empty}, extract::{Path, FromRequestParts, rejection::TypedHeaderRejectionReason}, async_trait, TypedHeader, headers::{Authorization, authorization::Bearer}, RequestPartsExt, Extension};

use bytes::Bytes;
use error::HandlerError;
use http::{StatusCode, request::Parts};
use memory_store::MemoryStore;
use mime_guess::mime::APPLICATION_JSON;

use site::{PresenceAnnouncement, Site, Presence};
use log::{trace, error};

use crate::oidc_cache::MetadataCache;


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
mod config;

const SWAGGER_SPEC_URL: &str = "/api/public/openapi.yaml";

fn init_logging() {
    let rust_log_config = config::get("rust_log").ok();
    let mut logger_builder = env_logger::builder();
    if let Some(rust_log) = rust_log_config {
        logger_builder.parse_filters(&rust_log);
    }
    logger_builder.init();
}

/// A simple Spin HTTP component.
#[tokio::main]
async fn main(){
    init_logging();

    let mut router: Router = Router::new()
    
    .route(SWAGGER_SPEC_URL, get(handle_get_swagger_spec))
    .route("/api/public/swagger-ui/:path", get(handle_get_swagger_ui))
    .route("/api/sites", get(handle_get_sites))
    .route("/api/sites/:siteId/presence", get(handle_get_sites_siteid_presence))
    .route("/api/sites/:siteId/hello", post(handle_post_sites_siteid_hello))
    .route("/api/sites/:siteId/announce", put(handle_put_announce))
    .route("/api/*", get(handle_get_fallback))
    .layer(Extension(MemoryStore::new()));
}

#[debug_handler]
async fn handle_get_fallback(OriginalUri(full_url): OriginalUri) -> Result<Redirect, HandlerError> {
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
async fn handle_get_sites_siteid_presence(_auth_info: AuthInfo, Path(site_id): Path<String>) -> Result<Json<Vec<Presence>>, HandlerError> {
    let presences = site::get_presence_on_site(&site_id)?;
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

async fn handle_post_sites_siteid_hello(auth_info: AuthInfo, Path(site_id): Path<String>) -> Result<Json<Vec<Presence>>, HandlerError> {

    let logged_as_name = to_logged_as_name(&auth_info);
    site::hello_site(&auth_info.subject, &logged_as_name, &site_id)?;

    // to return the current presences, proceed like with an ordinary
    // presence request
    return handle_get_sites_siteid_presence(auth_info, Path(site_id)).await

}

#[debug_handler]
async fn handle_put_announce(auth_info: AuthInfo, Path(site_id): Path<String>, Json(announcements): Json<Vec<PresenceAnnouncement>>) -> Result<impl IntoResponse, HandlerError> {

    site::announce_presence_on_site(&auth_info.subject, &site_id, &to_logged_as_name(&auth_info), &announcements)?;

    Ok(Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Empty::new())?
    )
}

async fn handle_get_sites(_auth_info: AuthInfo) -> Result<Json<Vec<Site>>, HandlerError> {
    Ok(Json(site::get_sites()?))
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
        if let Err(e) = ox.init(cache, &issuer_url) {
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

