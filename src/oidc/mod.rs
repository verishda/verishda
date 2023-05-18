
mod client_impl;

use anyhow::{anyhow, Result};

const ISSUER: &str = "https://lemur-5.cloud-iam.com/auth/realms/werischda";

use openidconnect::{
    AccessTokenHash,
    AuthenticationFlow,
    AuthorizationCode,
    ClientId,
    ClientSecret,
    CsrfToken,
    Nonce,
    IssuerUrl,
    PkceCodeChallenge,
    RedirectUrl,
    Scope, url, IdToken,
};
use openidconnect::core::{
  CoreAuthenticationFlow,
  CoreClient,
  CoreProviderMetadata,
  CoreResponseType,
  CoreUserInfoClaims,
};


use url::Url;

// Use OpenID Connect Discovery to fetch the provider metadata.
use openidconnect::{OAuth2TokenResponse, TokenResponse};



#[derive(Default)]
pub struct OidcExtension {
    config: Option<OidcConfig>,
}

struct OidcConfig {
    provider_metadata: CoreProviderMetadata,
    client: CoreClient,
}

impl OidcExtension {
    pub fn init(&mut self) -> anyhow::Result<()> {
        if self.config.is_none() {
println!("1");
            let issuer_url = IssuerUrl::new(ISSUER.to_string())?;
println!("2");
            let provider_metadata_result = CoreProviderMetadata::discover(
                &issuer_url,
                client_impl::exec,
            );
println!("3");
            let provider_metadata = match provider_metadata_result {
                Ok(m) => m,
                Err(e) => return {
                    let s = e.to_string();
                    Err(anyhow::Error::from(e))
                }
            };
println!("4");

            // Create an OpenID Connect client by specifying the client ID, client secret, authorization URL
            // and token URL.
            let client =
            CoreClient::from_provider_metadata(
                provider_metadata.clone(),
                ClientId::new("client_id".to_string()),
                Some(ClientSecret::new("client_secret".to_string())),
            )
            // Set the URL the user will be redirected to after the authorization process.
            .set_redirect_uri(RedirectUrl::new("http://redirect".to_string())?);
println!("5");

            self.config = Some(OidcConfig { provider_metadata, client });
        };
        Ok(())
    }

    pub fn check_auth_header(&self, auth_token: &str) -> Result<()> {
        let token = auth_token
            .to_ascii_lowercase()
            .strip_prefix("bearer ")
            .ok_or(anyhow!("Bearer "))?
            ;

        // FIXME: temporarily permanently unauthorized ;-)
        Err(anyhow!("unauthorized, always, for now."))

    }
}
