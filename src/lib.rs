use anyhow::{anyhow, Result};
use http::Extensions;
use site::PresenceAnnouncement;
use spin_sdk::{
    http::{Request, Response, Router, Params},
    http_component, config
};

struct AuthInfo {
    subject: String,
    given_name: Option<String>,
    family_name: Option<String>,
}

mod site;
mod oidc;

/// A simple Spin HTTP component.
#[http_component]
fn handle_hoozin_server(mut req: Request) -> Result<Response> {
    println!("{:?}", req.headers());

    let auth_info = if let Some(a) = check_authorization(&req)? {
        a
    } else {
        return Ok(http::Response::builder()
            .status(401)
            .body(None)?
        );
    };

    req.extensions_mut().insert(auth_info);

    let mut router = Router::new();
    router.get("/api/sites", handle_get_sites);
    router.get("/api/sites/:siteId/presence", handle_get_sites_siteid_presence);
    router.post("/api/sites/:siteId/hello", handle_post_sites_siteid_hello);
    router.put("/api/announce", handle_put_announce);
    router.any("/*", |_,_|Ok(http::Response::builder()
            .status(404)
            .header("foo", "bar")
            .body(Some("Hello, Fermyon".into()))?));
    router.handle(req)
}

fn filter_first_query_param<'a>(param_name: &str, query: &'a str) -> Option<&'a str> {
    query.split("&")
    .filter(|s|s.contains("="))
    .map(|s|{
        let mut sp = s.split("=");
        (sp.next().unwrap(), sp.next().unwrap())
    })
    .filter(|pair|pair.0.eq("site_id"))
    .map(|pair|pair.1)
    .next()
}

fn extract_site_param(params: &Params) -> Result<&str> {
    match params.get("siteId") {
        Some(site_id) => Ok(site_id),
        None => Err(anyhow!("siteId parameter not given")),
    }
}

fn handle_get_sites_siteid_presence(req: Request, params: Params) -> Result<Response> {
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

fn handle_post_sites_siteid_hello(req: Request, params: Params) -> Result<Response> {
    if !req.method().as_str().eq("POST") {
        return Ok(http::Response::builder()
            .status(405)
            .body(None)?
        );
    }
    let auth_info = extract_auth_info(&req)?;

    let site_id = extract_site_param(&params)?;

    let logged_as_name = auth_info.given_name
    .iter()
    .chain(auth_info.family_name.iter())
    .fold(String::new(), |a,s| a + " " + s.as_str())
    ;
    let logged_as_name = logged_as_name.trim();
    
    if let Err(e) = site::hello_site(&auth_info.subject, &logged_as_name, site_id) {
        return status400(&e.to_string())
    }

    // to return the current presences, proceed like with an ordinary
    // presence request
    return handle_get_sites_siteid_presence(req, params)

}

fn handle_put_announce(req: Request, params: Params) -> Result<Response> {
    if let Err(r) = check_http_methods(&req, &["PUT"]) {
        return Ok(r);
    }

    let bytes = req.body().as_ref().ok_or(anyhow!("no request body"))?;
    let announcements_str = String::from_utf8(bytes.to_vec())?;
    let announcements = serde_json::from_str::<Vec<PresenceAnnouncement>>(announcements_str.as_str())?;

    let auth_info = extract_auth_info(&req)?;

    site::announce_presence_on_site(&auth_info.subject, &announcements)?;

    let res = http::Response::builder()
    .status(200)
    .body(None)
    .unwrap();
    Ok(res)
}

fn handle_get_sites(_req: Request, _params: Params) -> Result<Response> {
    let sites = site::get_sites()?;
    let json_bytes = serde_json::ser::to_vec_pretty(&sites)?;
    Ok(http::Response::builder()
        .status(200)
        .body(Some(json_bytes.into()))?
    )
}

fn check_authorization(req: &Request) -> Result<Option<AuthInfo>> {
println!("A");
    let mut ox = oidc::OidcExtension::default();
    let issuer_url = config::get("issuer_url").or(Err(anyhow!("issuer_url not defined. Use a URL that can serve as a base URL for OIDC discovery")))?;
    ox.init(&issuer_url)?;
println!("B");
    let auth_token = match extract_auth_token(req) {
        Some(auth_token) => auth_token,
        None => return Ok(None)
    };
println!("C");
    match ox.check_auth_token(&auth_token) {
        Ok(auth_info) => Ok(Some(auth_info)),
        Err(e) => {
            println!("auth error: {}", e.to_string());
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

