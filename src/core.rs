use futures::prelude::*;
use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, http::uri, reqwest::async_http_client, ClientId, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge, RedirectUrl};
use anyhow::Result;
use tokio::{io::AsyncWriteExt, net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions}};
use tokio_util::codec::{FramedWrite, FramedRead, LengthDelimitedCodec};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};


#[derive(Default)]
pub struct AppCore {
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<()>,    // MISSING: actual credentials
    login_pipe_server: Option<NamedPipeServer>,
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

    pub fn start_login(&mut self) -> Result<String> {
        // start named pipe server
        let pipe_server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(Self::pipe_name())?;

        //self.login_pipe_server = Some(pipe_server);
        
        tokio::spawn(async move {
            Self::read_login_pipe_message(pipe_server).await;            
        });
        Ok(self.authorization_url())
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

    async fn read_login_pipe_message(mut pipe_server: NamedPipeServer) -> Result<()> {
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
                        break;
                    }
                }
            }
        }
        Ok(())
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