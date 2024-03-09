use std::sync::Arc;

use futures::prelude::*;
use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, reqwest::async_http_client, AuthorizationCode, ClientId, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl};
use anyhow::Result;
use tokio::{net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions}, sync::Mutex};
use tokio_util::codec::{FramedWrite, FramedRead, LengthDelimitedCodec};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use url::Url;

struct Credentials {
    access_token: String,
    refresh_token: String,
}

#[derive(Default)]
pub struct AppCore {
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<Credentials>,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum LoginPipeMessage {
    Cancel,
    HandleRedirect{
        code: String
    },
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

    fn pipe_name() -> String {
        format!("\\\\.\\pipe\\{}", Self::uri_scheme())
    }

    pub fn start_login<F>(app_core: Arc<Mutex<AppCore>>, finished_callback: F) -> Result<Url> 
    where F: FnOnce(Result<()>) + Send + 'static
    {
        let (auth_url, pkce_verifier) = app_core.blocking_lock().authorization_url();

        // start named pipe server
        let pipe_server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(Self::pipe_name())?;
        
        tokio::spawn(async move {
            if let Err(e) = Self::read_login_pipe_message(app_core, pkce_verifier, pipe_server).await {
                eprintln!("Error reading login pipe message: {}", e);
            }
        });
        Ok(auth_url)
    }

    pub fn cancel_login(&mut self) {
        tokio::spawn(async move {
            Self::write_pipe_message(LoginPipeMessage::Cancel).await.unwrap();
        });
    }

    async fn write_pipe_message(message: LoginPipeMessage) -> Result<()> {
        let mut pipe_client = ClientOptions::new()
        .open(Self::pipe_name())?;
        let frame = FramedWrite::new(&mut pipe_client, LengthDelimitedCodec::new());
        let mut writer = SymmetricallyFramed::new(frame, SymmetricalJson::default());
        writer.send(&message).await.unwrap();
        
        Ok(())
    }

    async fn read_login_pipe_message(app_core: Arc<Mutex<AppCore>>, pkce_verifier: PkceCodeVerifier, mut pipe_server: NamedPipeServer) -> Result<()> {
        pipe_server.connect().await?;

        let frame = FramedRead::new(&mut pipe_server, LengthDelimitedCodec::new());
        let mut reader = tokio_serde::SymmetricallyFramed::new(frame, SymmetricalJson::<LoginPipeMessage>::default());
        loop {
            if let Some(msg) = reader.try_next().await? {
                match msg {
                    LoginPipeMessage::Cancel => {
                        break;
                    }
                    LoginPipeMessage::HandleRedirect{code} => {
                        println!("Received authorization code: {}", code);
                        let (access_token, refresh_token) = Self::exchange_code_for_tokens(app_core.clone(), code, pkce_verifier).await?;
                        println!("Exchanged into access_token {access_token}");
                        app_core.lock().await.credentials = Some(Credentials {access_token, refresh_token});
                        
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    async fn exchange_code_for_tokens(app_core: Arc<Mutex<AppCore>>, code: String, pkce_verifier: PkceCodeVerifier) -> Result<(String, String)> {
        let app_core = app_core.lock().await;
        let client = app_core.oidc_client.as_ref().unwrap();
        let token_response = client.exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await?;
        let access_token = token_response.access_token().secret().to_string();
        let refresh_token = token_response.refresh_token().unwrap().secret().to_string();
        Ok((access_token, refresh_token))
    }

    pub async fn handle_login_redirect(url: &str) -> Result<()> {
        // parse url
        let url = url::Url::parse(url)?;

        // extract the authorization code
        let code = url.query_pairs()
            .find(|(key, _)| key == "code")
            .ok_or_else(|| anyhow::anyhow!("No authorization code in redirect URL"))?
            .1
            .to_string();

        Self::write_pipe_message(LoginPipeMessage::HandleRedirect { code }).await?;

        Ok(())
    }

    fn authorization_url(&self) -> (Url, PkceCodeVerifier) {
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

        (auth_url, pkce_verifier)
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