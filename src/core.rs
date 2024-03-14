use std::{sync::Arc, time::{Duration, Instant}};

use futures::prelude::*;
use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, reqwest::async_http_client, AuthorizationCode, ClientId, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken};
use anyhow::Result;
use reqwest::header::HeaderMap;
use tokio::{net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions}, sync::Mutex};
use tokio_util::codec::{FramedWrite, FramedRead, LengthDelimitedCodec};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use url::Url;
use winapi::vc::excpt;

use crate::client;

#[derive(Debug)]
struct Credentials {
    access_token: String,
    refresh_token: String,
    expires_at: Instant,
}

pub struct AppCore {
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<Credentials>,
    command_tx: tokio::sync::mpsc::Sender<AppCoreCommand>,
}

const PUBLIC_API_BASE_URL: &str = "https://verishda.shuttleapp.rs";

#[derive(serde::Serialize, serde::Deserialize)]
enum LoginPipeMessage {
    Cancel,
    HandleRedirect{
        code: String
    },
}

enum AppCoreCommand {
    RefreshPrecences,
    Quit,
}

impl AppCore {
    pub fn new() -> Arc<Mutex<Self>> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AppCoreCommand>(1);
        let app_core = Self {
            command_tx: tx,
            oidc_metadata: None,
            oidc_client: None,
            credentials: None,
        };

        let app_core = Arc::new(Mutex::new(app_core));
        let app_core_clone = app_core.clone();
        tokio::spawn(async move {
            let mut ival = tokio::time::interval(Duration::from_secs(5));
            ival.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = ival.tick() => {
                        app_core_clone.lock().await.refresh_presences().await;
                    }
                    cmd = rx.recv() => {
                        if let Some(cmd) = cmd {
                            match cmd {
                                AppCoreCommand::RefreshPrecences => {
                                    app_core_clone.lock().await.refresh_presences().await;
                                }
                                AppCoreCommand::Quit => {
                                    break;
                                }
                            }
                        }
                   }
                }
            }
        });
        app_core
    }

    /// The URI scheme name that is used to register the application as a handler for the redirect URL.
    pub fn uri_scheme() -> &'static str {
        "verishda"
    }

    /// Parameter that introduces the redirect url parameter on the command line.
    pub fn redirect_url_param() -> &'static str {
        "--redirect-url"
    }

    pub fn quit(&mut self) {
       self.command_tx.blocking_send(AppCoreCommand::Quit).unwrap();
    }

    async fn create_client(&mut self) -> Result<client::Client> {
        if let Some(credentials) = &mut self.credentials {
            if Instant::now().cmp(&credentials.expires_at) == std::cmp::Ordering::Greater{
                let refresh_token = RefreshToken::new(credentials.refresh_token.clone());
                let resp = self.oidc_client.as_ref().unwrap().exchange_refresh_token(&refresh_token)
                    .request_async(async_http_client)
                    .await?;
                credentials.access_token = resp.access_token().secret().to_string();
                credentials.expires_at = Self::expires_at_from_now(resp.expires_in());
            }

            let mut headers = HeaderMap::new();
            let access_token = &credentials.access_token;
            headers.insert("Authorization", format!("Bearer {access_token}").parse().unwrap());
            let inner = reqwest::Client::builder()
                .default_headers(headers)
                .connection_verbose(true)
                .build()
                .expect("client creation failed");
            let client = client::Client::new_with_client(PUBLIC_API_BASE_URL, inner);
            Ok(client)
        } else {
            Err(anyhow::anyhow!("Not logged in"))
        }
    }

    async fn refresh_presences(&mut self) {
        if let Ok(client) = self.create_client().await {
            
            match client.handle_get_sites().await {
                Ok(sites) => {
                    println!("Got sites: {:?}", sites);
                }
                Err(e) => {
                    println!("Failed to get sites: {}", e);
                }
            }
        }
    }
    fn redirect_url(&self) -> String {
        Self::uri_scheme().to_owned() + "://exchange-token"
    }

    fn pipe_name() -> String {
        format!("\\\\.\\pipe\\{}", Self::uri_scheme())
    }

    pub fn start_login<F>(app_core: Arc<Mutex<AppCore>>, finished_callback: F) -> Result<Url> 
    where F: FnOnce(bool) + Send + 'static
    {
        let (auth_url, pkce_verifier) = app_core.blocking_lock().authorization_url();

        // start named pipe server
        let pipe_server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(Self::pipe_name())?;
        
        tokio::spawn(async move {
            let r = Self::read_login_pipe_message(app_core, pkce_verifier, pipe_server).await;
            let logged_in = match r {
                Err(e) => {
                    println!("Error reading login pipe message: {}", e);
                    false
                },
                Ok(logged_in) => logged_in,
            };

            finished_callback(logged_in);
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

    async fn read_login_pipe_message(app_core: Arc<Mutex<AppCore>>, pkce_verifier: PkceCodeVerifier, mut pipe_server: NamedPipeServer) -> Result<bool> {
        pipe_server.connect().await?;

        let frame = FramedRead::new(&mut pipe_server, LengthDelimitedCodec::new());
        let mut reader = tokio_serde::SymmetricallyFramed::new(frame, SymmetricalJson::<LoginPipeMessage>::default());
        loop {
            if let Some(msg) = reader.try_next().await? {
                match msg {
                    LoginPipeMessage::Cancel => {
                        return Ok(false);
                    }
                    LoginPipeMessage::HandleRedirect{code} => {
                        println!("Received authorization code: {}", code);
                        let credentials = Self::exchange_code_for_tokens(app_core.clone(), code, pkce_verifier).await?;
                        println!("Exchanged into access_token {credentials:?}");
                        app_core.lock().await.credentials = Some(credentials);
                        
                        return Ok(true);
                    }
                }
            }
        }
    }

    fn expires_at_from_now(expires_in: Option<Duration>) -> Instant {
        let expires_in = expires_in
        .unwrap_or(Duration::from_secs(60));

        // reduce the expiration time by 10% to account for clock skew
        let expires_in = expires_in * 9 / 10;

        Instant::now() + expires_in
    }

    async fn exchange_code_for_tokens(app_core: Arc<Mutex<AppCore>>, code: String, pkce_verifier: PkceCodeVerifier) -> Result<Credentials> {
        let app_core = app_core.lock().await;
        let client = app_core.oidc_client.as_ref().unwrap();
        let token_response = client.exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await?;
        let access_token = token_response.access_token().secret().to_string();
        let refresh_token = token_response.refresh_token().unwrap().secret().to_string();
        let credentials = Credentials {
            access_token,
            refresh_token,
            expires_at: Self::expires_at_from_now(token_response.expires_in()),
        };
        Ok(credentials)
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