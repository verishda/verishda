use axum::response::{IntoResponse, Response};
use http::StatusCode;


pub struct HandlerError(anyhow::Error)
where Self: Send
;

impl IntoResponse for HandlerError {
    fn into_response(self) -> Response {
        let error = self.0;
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{error}")).into_response()
    }
}

impl<E> From<E> for HandlerError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}