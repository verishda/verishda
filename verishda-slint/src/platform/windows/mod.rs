mod uri_scheme;


pub fn startup(uri_scheme: &str, redirect_url_param: &str) {
    uri_scheme::register_custom_uri_scheme(uri_scheme, redirect_url_param).unwrap();
}

