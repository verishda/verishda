use anyhow::Result;
use spin_sdk::{
    http::{Request, Response, Router, Params},
    http_component
};

mod site;

/// A simple Spin HTTP component.
#[http_component]
fn handle_hoozin_server(req: Request) -> Result<Response> {
    println!("{:?}", req.headers());

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

fn status400(msg: &str) -> Result<Response> {
    Ok(http::Response::builder()
            .status(400)
            .body(Some(msg.to_owned().into()))?)
}

