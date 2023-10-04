use std::cell::OnceCell;

use anyhow::{anyhow, Result};
use mime_guess::mime::APPLICATION_JSON;
use site::PresenceAnnouncement;
use spin_sdk::{
    http::{Request, Response, Router, Params},
    http_component, config
};
use log::{info, trace, debug, error};

use crate::{spin_store::SpinStore, oidc_cache::MetadataCache};


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
mod spin_store;
mod oidc_cache;

fn init_logging() {
    let rust_log_config = spin_sdk::config::get("rust_log").ok();
    let mut logger_builder = env_logger::builder();
    if let Some(rust_log) = rust_log_config {
        logger_builder.parse_filters(&rust_log);
    }
    logger_builder.init();
}

/// A simple Spin HTTP component.
#[http_component]
fn handle_hoozin_server(mut req: Request) -> Result<Response> {
    init_logging();

    trace!("inbound request{:?}", req);

    
    if !req.uri().path().starts_with("/api/public/") && !req.uri().path().eq("/api") {
        let auth_info = if let Some(auth_info) = check_authorization(&req)? {
            auth_info
        } else {
            return Ok(http::Response::builder()
                .status(401)
                .body(None)?
            );
        };
        req.extensions_mut().insert(auth_info);
    };

    // only the spin-inserted header 'spin-full-url' appears to hold just that
    // the full URL for the call. We need the scheme and authority sections
    // to build the redirect url
    let spin_full_url = req.headers().get("spin-full-url").unwrap().to_owned();
    let spin_full_url = spin_full_url.to_str().unwrap().to_owned();

    let mut redirect_url = http::Uri::try_from(spin_full_url)?.into_parts();
    redirect_url.path_and_query = Some(http::uri::PathAndQuery::from_static("/api/public/swagger-ui/oauth2-redirect.html"));
    let redirect_url = http::Uri::from_parts(redirect_url).unwrap();
    

    let mut router = Router::new();

    let swagger_spec_url = "/api/public/openapi.yaml";
    let swagger_ui_url = format!("/api/public/swagger-ui/index.html?url={swagger_spec_url}&oauth2RedirectUrl={redirect_url}");
    router.get(swagger_spec_url, handle_get_swagger_spec);
    router.get("/api/public/swagger-ui/:path", handle_get_swagger_ui);
    router.get("/api/sites", handle_get_sites);
    router.get("/api/sites/:siteId/presence", handle_get_sites_siteid_presence);
    router.post("/api/sites/:siteId/hello", handle_post_sites_siteid_hello);
    router.put("/api/announce", handle_put_announce);
    router.any("/api", move |_,_|Ok(http::Response::builder()
            .status(302)
            .header("location", swagger_ui_url.clone())
            .body(None)?));
    router.handle(req)
}

#[allow(dead_code)] // we allow this until it is decided we don't need to filter for query params
fn filter_first_query_param<'a>(param_name: &str, query: &'a str) -> Option<&'a str> {
    query.split("&")
    .filter(|s|s.contains("="))
    .map(|s|{
        let mut sp = s.split("=");
        (sp.next().unwrap(), sp.next().unwrap())
    })
    .filter(|pair|pair.0.eq(param_name))
    .map(|pair|pair.1)
    .next()
}

fn extract_site_param(params: &Params) -> Result<&str> {
    match params.get("siteId") {
        Some(site_id) => Ok(site_id),
        None => Err(anyhow!("siteId parameter not given")),
    }
}

fn handle_get_swagger_spec(_req: Request, _params: Params) -> Result<Response> {
    let resp = http::Response::builder()
    .status(200)
    .body(Some(bytes::Bytes::copy_from_slice(SWAGGER_SPEC.get_or_init(||swagger_ui::swagger_spec_file!("./verishda.yaml")).content)))
    ?;

    Ok(resp)
}

fn handle_get_swagger_ui(_req: Request, params: Params) -> Result<Response>{

    let path = params.get("path").unwrap();

    let resp = match swagger_ui::Assets::get(path) {
        Some(data) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            http::Response::builder()
            .status(200)
            .header("Content-Type", mime.to_string())
            .body(Some(bytes::Bytes::copy_from_slice(data.as_ref())))?
        },
        None => http::Response::builder()
            .status(404)
            .body(Some(bytes::Bytes::from("404 Not Found".as_bytes())))?
    };

    Ok(resp)
}

fn handle_get_sites_siteid_presence(_req: Request, params: Params) -> Result<Response> {
    let site_id = extract_site_param(&params)?;
    let presences = site::get_presence_on_site(site_id)?;
    let json_bytes = serde_json::ser::to_vec_pretty(&presences)?;
    Ok(http::Response::builder()
        .status(200)
        .body(Some(json_bytes.into()))?
    )
}

fn check_http_methods(req: &Request, methods: &[&str]) -> Result<(), Response> {
    for method in methods {
        if *method == req.method().as_str() {
            return Ok(());
        }
    }
    let msg = format!("given http method not allowed. Allowed methods: {:?}", methods);
    let res = http::Response::builder()
    .status(405)
    .body(Some(msg.into()))
    .unwrap();
    return Err(res);
}

fn extract_auth_info(req: &Request) -> Result<&AuthInfo> {
    req.extensions().get::<AuthInfo>().ok_or(anyhow!("failed to extract authentication info from request"))
}

fn to_logged_as_name(auth_info: &AuthInfo) -> String {
    auth_info.given_name
    .iter()
    .chain(auth_info.family_name.iter())
    .fold(String::new(), |a,s| a + " " + s.as_str())
    .trim()
    .to_string()
}

fn handle_post_sites_siteid_hello(req: Request, params: Params) -> Result<Response> {
    if !req.method().as_str().eq("POST") {
        return Ok(http::Response::builder()
            .status(405)
            .body(None)?
        );
    }
    let auth_info = extract_auth_info(&req)?;

    let site_id = extract_site_param(&params)?;

    let logged_as_name = to_logged_as_name(auth_info);
    if let Err(e) = site::hello_site(&auth_info.subject, &logged_as_name, site_id) {
        return status400(&e.to_string())
    }

    // to return the current presences, proceed like with an ordinary
    // presence request
    return handle_get_sites_siteid_presence(req, params)

}

fn handle_put_announce(req: Request, _params: Params) -> Result<Response> {
    let auth_info = extract_auth_info(&req)?;

    let bytes = req.body().as_ref().ok_or(anyhow!("no request body"))?;
    let announcements_str = String::from_utf8(bytes.to_vec())?;
    let announcements = serde_json::from_str::<Vec<PresenceAnnouncement>>(announcements_str.as_str())?;


    site::announce_presence_on_site(&auth_info.subject, &to_logged_as_name(auth_info), &announcements)?;

    Ok(http::Response::builder()
        .status(204)
        .header("Content-Type", APPLICATION_JSON.as_ref())
        .body(None)?)
}

fn handle_get_sites(_req: Request, _params: Params) -> Result<Response> {
    let sites = site::get_sites()?;
    let json_bytes = serde_json::ser::to_vec_pretty(&sites)?;
    Ok(http::Response::builder()
        .status(200)
        .header("Content-Type", APPLICATION_JSON.as_ref())
        .body(Some(json_bytes.into()))?
    )
}

fn check_authorization(req: &Request) -> Result<Option<AuthInfo>> {
    trace!("checking authorization...");
    let mut ox = oidc::OidcExtension::default();
    let issuer_url = config::get("issuer_url").or(Err(anyhow!("issuer_url not defined. Use a URL that can serve as a base URL for OIDC discovery")))?;
    let cache = MetadataCache::new(SpinStore::new(spin_sdk::key_value::Store::open_default()?));
    ox.init(cache, &issuer_url)?;
    let auth_token = extract_auth_token(req);
    trace!("auth_token: {auth_token:?}");
    let auth_token = match extract_auth_token(req) {
        Some(auth_token) => auth_token,
        None => return Ok(None)
    };

    let auth_info_opt = ox.check_auth_token(&auth_token);
    trace!("auth_info {auth_info_opt:?}");
    match auth_info_opt {
        Ok(auth_info) => Ok(Some(auth_info)),
        Err(e) => {
            error!("auth error: {e}");
            Ok(None)
        }
    }
}

fn extract_auth_token(req: &Request) -> Option<String> {
    let auth_header = req.headers()
    .get("Authorization");
    let auth_header = match auth_header {
        Some(h) => String::from_utf8(h.as_bytes().into()).unwrap(),
        None => return None
    };

    auth_header.strip_prefix("Bearer ")
    .map(|h|h.trim())
    .map(|h|String::from(h))
}


fn status400(msg: &str) -> Result<Response> {
    Ok(http::Response::builder()
            .status(400)
            .body(Some(msg.to_owned().into()))?)
}

