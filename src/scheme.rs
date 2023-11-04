use axum::{async_trait, extract::FromRequestParts, response::IntoResponse, body::{BoxBody, Full, Empty}};
use bytes::Bytes;
use http::{StatusCode, request::Parts};


/// Extractor which resolves the URI scheme used for the request.
/// 
/// Reads the 'X-Forwarded-Proto' header. In the future, it may also
/// read the 'Forwarded' header (it does not at the moment)
#[derive(Clone,Debug)]
pub struct Scheme(pub String);


#[async_trait]
impl<S> FromRequestParts<S> for Scheme
where
    S: Send + Sync,
{
    type Rejection = ();

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match parts.headers.get("X-Forwarded-Proto") {
            Some(x_forwarded_proto) => Ok(Self(x_forwarded_proto.to_str().unwrap_or("http").into())),
            None => Ok(Self("http".to_string()))
        }    
    }
}