
mod client_impl;

use anyhow::{anyhow, Result};
use jwt_simple::prelude::*;
use serde::Deserialize;

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
    Scope, url, IdToken, JsonWebKey, JsonWebKeyId,
};
use openidconnect::core::{
  CoreAuthenticationFlow,
  CoreClient,
  CoreProviderMetadata,
  CoreResponseType,
  CoreUserInfoClaims, CoreJsonWebKeyUse, CoreJwsSigningAlgorithm,
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

    pub fn check_auth_token(&self, token_str: &str) -> Result<()> {


println!("b: {}", token_str);
        let token_metadata = Token::decode_metadata(token_str)?;
println!("c");
        let config = self.config.as_ref().unwrap();
        let supported_algos = config.provider_metadata.id_token_signing_alg_values_supported();
//        let token_algo = serde_json::from_str::<CoreJwsSigningAlgorithm>(token_metadata.algorithm())?;
let token_algo = CoreJwsSigningAlgorithm::deserialize(serde_json::to_value(token_metadata.algorithm())?)?;
println!("d");
        if !supported_algos.contains(&token_algo) || token_algo == CoreJwsSigningAlgorithm::None {
            return Err(anyhow!("token algorithm {:?} not supported", token_algo));
        }
println!("e");
        let token_key_id = token_metadata.key_id().ok_or(anyhow!("token contains no key_id"))?;
println!("f");
        
        let key = config.provider_metadata.jwks().keys().iter()
            .filter(|k|k.key_use() == Some(&CoreJsonWebKeyUse::Signature))
            .filter(|k|k.key_id() == Some(&JsonWebKeyId::new(String::from(token_key_id))))
            .next()
            .ok_or(anyhow!("no matching key found"))?;


println!("e");
        let token_message_signature = token_str
        .rsplit_once('.')
        .ok_or(anyhow!("signature component not found"))?;
        let token_signature = Base64UrlSafeNoPadding::decode_to_vec(token_message_signature.1, None)?;

println!("f");

        let (token_message, token_signature) = (token_message_signature.0, &token_signature);
        
        key.verify_signature(&token_algo, token_message.as_bytes(), token_signature)?;

        Ok(())
    }
}

