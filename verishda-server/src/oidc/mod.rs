
//mod client_impl;

use std::str::FromStr;

use crate::store::Cache;
use openidconnect::reqwest::async_http_client;


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


async fn fetch_metadata(issuer_url: &str) -> Result<CoreProviderMetadata, anyhow::Error> {
    trace!("acquiring provider metadata via OIDC discovery...");
    let issuer_url = IssuerUrl::new(issuer_url.to_string())?;
    let provider_metadata_result = CoreProviderMetadata::discover_async(
        issuer_url,
        async_http_client,
    ).await;
    trace!("discovery result received.");
    let provider_metadata = match provider_metadata_result {
        Ok(m) => m,
        Err(e) => {
            error!("disovery result in error: {e}");
            return Err(anyhow::Error::from(e))
        }
    };
    trace!("provider metadata loaded successfully: {provider_metadata:?}");

    Ok(provider_metadata)
}

const OIDC_METADATA_KEY: &str = "oidc_metadata";

impl OidcExtension {
    pub async fn init(&mut self, mut cache: impl Cache<str, CoreProviderMetadata>, issuer_url: &str) -> anyhow::Result<()> {
        if self.config.is_none() {
            trace!("having no OIDC config, initializing..");
            let provider_metadata = match cache.get(OIDC_METADATA_KEY) {
                Some(m) => m,
                None => {
                    let m = fetch_metadata(issuer_url).await?;
                    cache.set(OIDC_METADATA_KEY, m.clone())?;
                    m
                }
            };

            trace!("OIDC provider metadata: {provider_metadata:?}");

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

    pub(crate) fn check_auth_token(&self, token_str: &str) -> anyhow::Result<AuthInfo> {

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
