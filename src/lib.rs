use anyhow::{anyhow, Result};
use spin_sdk::{
    http::{Request, Response, Router, Params},
    http_component
};

struct AuthInfo;

mod site;
mod oidc;

/// A simple Spin HTTP component.
#[http_component]
fn handle_hoozin_server(req: Request) -> Result<Response> {
    println!("{:?}", req.headers());

    let auth_info = if let Some(a) = check_authorization(&req)? {
        a
    } else {
        return Ok(http::Response::builder()
            .status(401)
            .body(None)?
        );
    };

    let mut router = Router::new();
    router.get("/api/sites", handle_get_sites);
    router.post("/api/sites/:siteId/hello", handle_post_sites_siteid_hello);
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

fn handle_post_sites_siteid_hello(req: Request, params: Params) -> Result<Response> {
    if !req.method().as_str().eq("POST") {
        return Ok(http::Response::builder()
            .status(504)
            .body(None)?
        );
    }
    let user_id = "affe32";
    let site_id = if let Some(site_id_str) = params.get("siteId") {
        if let Ok(i) = i32::from_str_radix(site_id_str, 10) {
            i
        } else {
            return status400("siteId parameter must be integer")
        }
    } else {
        return status400("no site specified".into())
    };

    let status = match site::hello_site(&user_id, site_id) {
        Ok(_) => 200,
        Err(_) => 400
    };

    Ok(http::Response::builder()
        .status(status).body(None)?
    )

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
    let issuer_url = std::env::var("ISSUER_URL").or(Err(anyhow!("ISSUER_URL not defined. Use a URL that can serve as a base URL for OIDC discovery")))?;
    ox.init(&issuer_url)?;
println!("B");
    let auth_token = match extract_auth_token(req) {
        Some(auth_token) => auth_token,
        None => return Ok(None)
    };
println!("C");
    match ox.check_auth_token(&auth_token) {
        Ok(_) => Ok(Some(AuthInfo{})),
        Err(e) => Err(e)
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

