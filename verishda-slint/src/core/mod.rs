use std::{path::PathBuf, sync::Arc, time::{Duration, Instant}};

use chrono::Days;
use futures::prelude::*;
use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, reqwest::async_http_client, AuthorizationCode, ClientId, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken};
use anyhow::Result;

use reqwest::header::HeaderMap;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use tokio_util::codec::{FramedWrite, FramedRead, LengthDelimitedCodec};
use tokio_serde::{formats::SymmetricalJson, SymmetricallyFramed};
use url::Url;
use log::*;

use verishda_dto::{self, types::{PresenceAnnouncement, PresenceAnnouncementKind, PresenceAnnouncements}};
use crate::core::location::Location;

mod location;

const PUBLIC_ISSUER_URL: &str = "https://lemur-5.cloud-iam.com/auth/realms/werischda";
const PUBLIC_CLIENT_ID: &str = "verishda-windows";

#[derive(Default, Clone, Debug)]
pub enum Announcement {
    #[default]
    NotAnnounced,
    PresenceAnnounced,
    WeeklyPresenceAnnounced,
}

#[derive(Debug, Clone)]
struct Credentials {
    access_token: String,
    refresh_token: String,
    expires_at: Instant,
}

pub struct AppCore {
    location_handler: Arc<Mutex<location::LocationHandler>>,
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<Credentials>,
    command_tx: tokio::sync::mpsc::Sender<AppCoreCommand>,
    on_core_event: Option<Box<dyn Fn(CoreEvent) + Send>>,
    site: Option<String>,
}

pub enum CoreEvent {
    SitesUpdated(Vec<verishda_dto::types::Site>),
    PresencesChanged(Vec<verishda_dto::types::Presence>),
}

const PUBLIC_API_BASE_URL: &str = "https://verishda.shuttleapp.rs";
//const PUBLIC_API_BASE_URL: &str = "http://127.0.0.1:3000";

#[derive(serde::Serialize, serde::Deserialize)]
enum LoginPipeMessage {
    Cancel,
    HandleRedirect{
        code: String
    },
}

enum AppCoreCommand {
    RefreshPrecences,
    PublishAnnouncements{
        site_id: String,
        announcements: Vec<Announcement>
    },
    Quit,
}

impl AppCore {
    pub fn new() -> Arc<Mutex<Self>> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AppCoreCommand>(1);
        let app_core = Self {
            location_handler: location::LocationHandler::new(),
            command_tx: tx,
            oidc_metadata: None,
            oidc_client: None,
            credentials: None,
            on_core_event: None,
            site: None,
        };

        let app_core = Arc::new(Mutex::new(app_core));
        let app_core_clone = app_core.clone();
        tokio::spawn(async move {

            log::info!("AppCore background task started");

            // start with refreshing presences
            app_core_clone.lock().await.refresh_sites().await;

            // install interval timer
            let mut site_refresh_ival = tokio::time::interval(Duration::from_secs(5*60));
            let mut presence_refresh_ival = tokio::time::interval(Duration::from_secs(1*60));
            site_refresh_ival.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            
            loop {
                tokio::select! {
                    _ = site_refresh_ival.tick() => {
                        app_core_clone.lock().await.refresh_sites().await;
                    }
                    _ = presence_refresh_ival.tick() => {
                        app_core_clone.lock().await.update_own_presence().await;
                        app_core_clone.lock().await.refresh_presences().await;
                    }
                    cmd = rx.recv() => {
                        if let Some(cmd) = cmd {
                            match cmd {
                                AppCoreCommand::RefreshPrecences => {
                                    app_core_clone.lock().await.update_own_presence().await;
                                    app_core_clone.lock().await.refresh_presences().await;
                                },
                                AppCoreCommand::PublishAnnouncements{site_id, announcements} => {
                                    app_core_clone.lock().await.publish_own_announcements(site_id, announcements).await;
                                },
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

    pub fn set_site(&mut self, site_id: &str) {
        let new_site = if site_id.is_empty() {
           None
        } else {
           Some(site_id.to_string())
        };
        let changed = self.site != new_site;
        self.site = new_site;
        if changed {
            _ = self.command_tx.blocking_send(AppCoreCommand::RefreshPrecences);
        }
    }

    pub fn refresh(&self) {
        _ = self.command_tx.blocking_send(AppCoreCommand::RefreshPrecences);
    }

    pub fn announce(&self, site_id: String, announcements: Vec<Announcement>) {
        _ = self.command_tx.blocking_send(AppCoreCommand::PublishAnnouncements{
            site_id, 
            announcements: announcements.clone()
        });
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

    async fn create_client(&mut self) -> Result<verishda_dto::Client> {
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
            let client = verishda_dto::Client::new_with_client(PUBLIC_API_BASE_URL, inner);
            Ok(client)
        } else {
            Err(anyhow::anyhow!("Not logged in"))
        }
    }

    async fn refresh_sites(&mut self) {
        log::trace!("Refreshing sites");
        if let Ok(client) = self.create_client().await {
            
            match client.handle_get_sites().await {
                Ok(sites_response) => {
                    let sites = sites_response.into_inner();
                    log::debug!("Got sites: {sites:?}", );
                    let mut location_handler = self.location_handler.lock().await;
                    location_handler.clear_geofences();
                    for site in &sites {
                        let location = Location::new(site.latitude as f64, site.longitude as f64);
                        let _ = location_handler.add_geofence_circle(&site.id, &location, 100.);
                    }
                    self.broadcast_core_event(CoreEvent::SitesUpdated(sites));
                }
                Err(e) => {
                    println!("Failed to get sites: {}", e);
                }
            }
        }
    }

    async fn update_own_presence(&mut self) {
        if let Ok(client) = self.create_client().await {
            // note: the geo fence IDs are are set as the site IDs
            for site_id in self.location_handler.lock().await.get_occupied_geofences() {
                if let Err(e) = client.handle_post_sites_siteid_hello(&site_id).await {
                    log::error!("Failed to update presence for site {site_id}: {e}")
                }
            }
        }
    }

    async fn refresh_presences(&mut self) {
        if let Ok(client) = self.create_client().await {

            log::trace!("Refreshing presences");
            let site = if let Some(site) = &self.site {
                site
            } else {
                log::trace!("No site selected, aborting presence refresh");
                return;
            };

            log::trace!("Getting presences for site {site}");
            match client.handle_get_sites_siteid_presence(site).await {
                Ok(sites_response) => {
                    let presences = sites_response.into_inner();
                    println!("Got presences: {:?}", presences);
                    self.broadcast_core_event(CoreEvent::PresencesChanged(presences));
                }
                Err(e) => {
                    println!("Failed to get sites: {}", e);
                }
            }
        }
    }

    async fn publish_own_announcements(&mut self, site_id: String, announcements: Vec<Announcement>) {
        if let Ok(client) = self.create_client().await {
            let now_date = chrono::Utc::now().naive_utc().date();
            debug!("{announcements:?}");
            let announcements = announcements.iter()
                .enumerate()
                .map(|(days_from_now,a)|{
                    let date = now_date
                    .checked_add_days(Days::new(days_from_now as u64))
                    .unwrap_or(now_date);

                    let kind = match a {
                        Announcement::WeeklyPresenceAnnounced => 
                            PresenceAnnouncementKind::RecurringAnnouncement,
                        Announcement::PresenceAnnounced => 
                            PresenceAnnouncementKind::SingularAnnouncement,
                        Announcement::NotAnnounced => 
                            return None
                    };

                    Some(PresenceAnnouncement{
                        kind,
                        date
                    })
                })
                .filter_map(|o|o)
                .collect();
            
            if let Err(e) = client.handle_put_announce(&site_id, &PresenceAnnouncements(announcements)).await {
                log::error!("error while reporting announcement: {e}");
            }
        }
    }

    pub fn on_core_event<F>(&mut self, f: F)
    where F: Fn(CoreEvent) + Send + 'static
    {
        self.on_core_event = Some(Box::new(f));
    }

    fn broadcast_core_event(&self, event: CoreEvent) {
        if let Some(on_core_event) = &self.on_core_event {
            on_core_event(event);
        }
    }

    fn redirect_url(&self) -> String {
        Self::uri_scheme().to_owned() + "://exchange-token"
    }

    fn setings_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap();
        let home = std::path::PathBuf::from(home);
        let settings_path = home.join(format!(".{}", Self::uri_scheme()));
        settings_path
    }

    fn pipe_path() -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("\\\\.\\pipe\\{}", Self::uri_scheme()))
        }
        #[cfg(unix)]
        {
            let mut settings_path = Self::setings_path();
            std::fs::create_dir(&settings_path);
            settings_path.join("login")
        }
    }

    pub fn start_login<F>(app_core: Arc<Mutex<AppCore>>, finished_callback: F) -> Result<Url> 
    where F: FnOnce(bool) + Send + 'static
    {
        let (auth_url, pkce_verifier) = app_core.blocking_lock().authorization_url();

        // start named pipe server
        let pipe_server;
        #[cfg(windows)]
        {
            pipe_server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(Self::pipe_path())?;
        }

        
        #[cfg(unix)] 
        {
            pipe_server = UnixListener::bind(Self::pipe_path())?
        }      
        
        tokio::spawn(async move {
            log::info!("Waiting for login pipe message");

            let pipe;
            #[cfg(windows)]
            {
                if let Err(e) = pipe_server.connect().await {
                    log::error!("could not connect to named pipe: {e}");
                    finished_callback(false);
                    return;
                } else {
                    pipe = pipe_server;
                }
            };
            #[cfg(unix)]
            {
                pipe = match pipe_server.accept().await {
                    Err(e) => {
                        log::error!("failed to connect to unix socket");
                        finished_callback(false);
                        return;
                    }
                    Ok((pipe,_)) => {
                        pipe
                    }
                };
            };

            let r = Self::read_login_pipe_message(pipe).await;

            let r = match r{
                Ok(Some(msg)) => Self::evaluate_login_message(app_core.clone(), msg, pkce_verifier).await,
                Ok(None) => Ok(false),
                Err(e) => Err(e),
            };

            let logged_in = match r {
                Err(e) => {
                    log::error!("Error reading login pipe message: {}", e);
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
        let mut pipe_client;

        #[cfg(windows)]
        {
            pipe_client = ClientOptions::new()
            .open(Self::pipe_path())?;
        }

        #[cfg(unix)]
        {
            pipe_client = UnixStream::connect(Self::pipe_path()).await?;
        }
        let frame = FramedWrite::new(&mut pipe_client, LengthDelimitedCodec::new());
        let mut writer = SymmetricallyFramed::new(frame, SymmetricalJson::default());
        writer.send(&message).await.unwrap();
        
        Ok(())
    }

    async fn read_login_pipe_message(pipe_server: impl tokio::io::AsyncRead) -> Result<Option<LoginPipeMessage>> {
        let frame = FramedRead::new(pipe_server, LengthDelimitedCodec::new());
        let reader = tokio_serde::SymmetricallyFramed::new(frame, SymmetricalJson::<LoginPipeMessage>::default());
        tokio::pin!(reader);
        loop {
            if let Some(msg) = reader.try_next().await? {
                return Ok(Some(msg))
            }
        }
    }

    async fn evaluate_login_message(app_core: Arc<Mutex<AppCore>>, msg: LoginPipeMessage, pkce_verifier: PkceCodeVerifier) -> anyhow::Result<bool> {
        match msg {
            LoginPipeMessage::Cancel => {
                return Ok(false);
            }
            LoginPipeMessage::HandleRedirect{code} => {
                log::info!("Received authorization code: {}", code);
                let credentials = Self::exchange_code_for_tokens(app_core.clone(), code, pkce_verifier).await?;
                log::info!("Exchanged into access_token {credentials:?}");
                app_core.lock().await.credentials = Some(credentials);
                app_core.lock().await.refresh_sites().await;
                return Ok(true);
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
        let (auth_url, _csrf_token, _nonce) = self.oidc_client.as_ref().unwrap()
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

    pub async fn init(&mut self) -> Result<()>{
        self.init_provider(PUBLIC_ISSUER_URL, PUBLIC_CLIENT_ID).await?;
        Ok(())
    }

    async fn init_provider(&mut self, issuer_url: &str, client_id: &str) -> Result<()>{
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