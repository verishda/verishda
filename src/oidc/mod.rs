
mod client_impl;

use std::str::FromStr;

use anyhow::{Result};


use openidconnect::{
    ClientId,
    ClientSecret,
    Nonce,
    IssuerUrl,
    RedirectUrl,
    NonceVerifier,
};
use openidconnect::core::{
  CoreClient,
  CoreProviderMetadata,
  CoreIdToken,
};

use crate::AuthInfo;

use log::{trace, error};


#[derive(Default)]
pub struct OidcExtension {
    config: Option<OidcConfig>,
}

struct OidcConfig {
    _provider_metadata: CoreProviderMetadata,
    client: CoreClient,
}

impl OidcExtension {
    pub fn init(&mut self, issuer_url: &str) -> anyhow::Result<()> {
        if self.config.is_none() {
            trace!("having no OIDC config, initializing..");
            let issuer_url = IssuerUrl::new(issuer_url.to_string())?;
            trace!("acquiring provider metadata via OIDC discovery...");
            let provider_metadata_result = CoreProviderMetadata::discover(
                &issuer_url,
                client_impl::exec,
            );
            trace!("discovery result received.");
            let provider_metadata = match provider_metadata_result {
                Ok(m) => m,
                Err(e) => {
                    error!("disovery result in error: {e}");
                    return Err(anyhow::Error::from(e))
                }
            };
            trace!("provider metadata loaded successfully: {provider_metadata:?}");

            // Create an OpenID Connect client by specifying the client ID, client secret, authorization URL
            // and token URL.
            let client =
            CoreClient::from_provider_metadata(
                provider_metadata.clone(),
                ClientId::new("account".to_string()),
                Some(ClientSecret::new("client_secret".to_string())),
            )
            // Set the URL the user will be redirected to after the authorization process.
            .set_redirect_uri(RedirectUrl::new("http://redirect".to_string())?);
            trace!("OIDC client created successfully from provider metadata");

            self.config = Some(OidcConfig { _provider_metadata: provider_metadata, client });
        };
        Ok(())
    }

    pub(crate) fn check_auth_token(&self, token_str: &str) -> Result<AuthInfo> {

        // at this point we assume the access token is a JWT (like Keycloak and probably other IDPs encode their access tokens)
        let token = CoreIdToken::from_str(token_str)?;
        let config = &self.config.as_ref().unwrap();
        let claims = token.claims(&config.client.id_token_verifier(), WaiveNonceVerifier{})?;
        Ok(AuthInfo{
            subject: claims.subject().to_string(),
            given_name: claims.given_name()
            .and_then(|lc|lc.get(None))
            .map(|n|n.to_string()),
            family_name: claims.family_name()
            .and_then(|lc|lc.get(None))
            .map(|n|n.to_string()),
        })
    }
}

struct WaiveNonceVerifier{}

impl NonceVerifier for WaiveNonceVerifier {
    fn verify(self, _nonce: Option<&Nonce>) -> std::result::Result<(), String> {
        Ok(())
    }
}
