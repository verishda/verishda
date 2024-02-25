use axum::{async_trait, extract::FromRequestParts};
use http::request::Parts;
use crate::VerishdaState;


/// Extractor which resolves the URI scheme used for the request.
/// 
/// Reads the 'X-Forwarded-Proto' header. In the future, it may also
/// read the 'Forwarded' header (it does not at the moment)
#[derive(Clone,Debug)]
pub struct Scheme(pub String);


#[async_trait]
impl FromRequestParts<VerishdaState> for Scheme
{
    type Rejection = ();

    async fn from_request_parts(parts: &mut Parts, state: &VerishdaState) -> Result<Self, Self::Rejection> {
        let mut detected_scheme = None;
        if let Ok(forwared_proto_config) = state.config.get("FORWARDED_PROTO") {
            detected_scheme = Some(forwared_proto_config);
        }
        if detected_scheme.is_none() {
            if let Some(x_forwarded_proto) = parts.headers.get("X-Forwarded-Proto") {
                detected_scheme = x_forwarded_proto.to_str().ok().map(|s| s.to_string());
            }
        }

        let scheme = match detected_scheme {
            Some(s) => s.to_string(),
            None => "http".to_string()
        };
        
        Ok(Self(scheme))
    }
}