use std::{fmt::Display, sync::{Arc, Mutex}};

use anyhow::Result;
use ::http::uri::InvalidUri;
use thiserror::Error;

use openidconnect;

use spin_sdk::http;



#[derive(Error, Debug)]
//pub(crate) struct OidcClientError(Mutex<Arc<dyn std::error::Error>>);
pub(crate) enum OidcClientError {
    #[error("{0}")]
    InvalidUri(InvalidUri),
    #[error("{0:?}")]
    HttpCommsError(String),
}

pub(crate) fn exec(oidc_req: openidconnect::HttpRequest) -> Result<openidconnect::HttpResponse, OidcClientError> 
{

    let body = if oidc_req.body.is_empty() {
        None
    } else {
        Some(oidc_req.body.into())
    };

    let mut req = http::Request::new(body);
    *req.uri_mut() = match oidc_req.url.as_str().parse() {
        Ok(uri) => uri,
        Err(e) => return Err(OidcClientError::InvalidUri(e))
    };
    let http_res = match http::send(req) {
        Ok(http_res) => http_res,
        Err(e) => return Err(OidcClientError::HttpCommsError(e.to_string()))
    };

    Ok(openidconnect::HttpResponse{
        status_code: http_res.status(),
        headers: http_res.headers().clone(),
        body: match http_res.body() {
            Some(body) => body.clone().into(),
            None => Vec::new().into()
        },
    })
}

