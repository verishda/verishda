use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, http::uri, reqwest::async_http_client, ClientId, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge, RedirectUrl};
use anyhow::Result;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

#[derive(Default)]
pub struct AppCore {
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<()>,    // MISSING: actual credentials
    login_pipe_server: Option<NamedPipeServer>,
}

impl AppCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The URI scheme name that is used to register the application as a handler for the redirect URL.
    pub fn uri_scheme() -> &'static str {
        "verishda"
    }

    /// Parameter that introduces the redirect url parameter on the command line.
    pub fn redirect_url_param() -> &'static str {
        "--redirect-url"
    }

    fn redirect_url(&self) -> String {
        Self::uri_scheme().to_owned() + "://exchange-token"
    }

    pub fn start_login(&mut self) -> Result<String> {
        // start named pipe server
        let pipe_server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(format!(r"\\.\pipe\{}", Self::uri_scheme()))?;

        self.login_pipe_server = Some(pipe_server);
        
        Ok(self.authorization_url())
    }

    pub fn cancel_login(&mut self) {
        self.login_pipe_server = None;
    }

    fn authorization_url(&self) -> String {
        // Generate a PKCE challenge.
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    // Generate the full authorization URL.
    let (auth_url, csrf_token, nonce) = self.oidc_client.as_ref().unwrap()
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        // Set the PKCE code challenge.
        .set_pkce_challenge(pkce_challenge)
        .url();

        auth_url.to_string()
    }

    pub async fn init_provider(&mut self, issuer_url: &str, client_id: &str) -> Result<()>{
        let issuer_url = IssuerUrl::new(issuer_url.to_string()).unwrap();
        let redirect_url = RedirectUrl::new(self.redirect_url())?;
        
        self.oidc_metadata = Some(CoreProviderMetadata::discover_async(
            issuer_url,
            async_http_client,
        ).await?);

        let client_id = ClientId::new(client_id.to_string());
        let client = CoreClient::from_provider_metadata(
            self.oidc_metadata.as_ref().unwrap().clone(),
            client_id,
            None,
        )
        // Set the URL the user will be redirected to after the authorization process.
        .set_redirect_uri(redirect_url);
        
        self.oidc_client = Some(client);
    


        Ok(())
    }
}