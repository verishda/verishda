mod uri_scheme;
mod open_url;

pub use open_url::open_url;

pub fn startup(uri_scheme: &str, redirect_url_param: &str) {
    uri_scheme::register_custom_uri_scheme(uri_scheme, redirect_url_param).unwrap();
}

