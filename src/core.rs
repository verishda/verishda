use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, http::uri, reqwest::async_http_client, ClientId, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge, RedirectUrl};
use anyhow::Result;

pub struct AppCore {
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<()>,    // MISSING: actual credentials
}

impl AppCore {
    pub fn new() -> Self {
        Self {
            oidc_metadata: None,
            oidc_client: None,
            credentials: None,
        }
    }

    pub fn uri_scheme() -> &'static str {
        "verishda"
    }

    fn redirect_url(&self) -> String {
        Self::uri_scheme().to_owned() + "://exchange-token"
    }

    pub fn authorization_url(&self) -> String {
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