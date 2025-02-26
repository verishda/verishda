use std::{sync::{mpsc::RecvError, Arc}, time::{Duration, Instant}};

use chrono::Days;
use futures::prelude::*;
use location::LocationHandler;
use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, reqwest::async_http_client, AuthorizationCode, ClientId, CsrfToken, ExtraTokenFields, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, StandardTokenResponse, TokenResponse, TokenType};
use anyhow::Result;

use reqwest::header::HeaderMap;
use tokio::{sync::{mpsc::Sender, Mutex, Notify}, time::MissedTickBehavior};
use url::Url;
use log::*;

use verishda_config::Config;
use verishda_dto::types::{PresenceAnnouncement, PresenceAnnouncementKind, PresenceAnnouncements};
use crate::core::location::Location;

mod location;
pub mod startup;
pub mod verishda_dto;

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

#[derive(Default, Debug)]
pub struct PersonFilter {
    pub favorites_only: bool,
    pub term: Option<String>,
}

#[derive(Default, Debug)]
pub struct Settings {
    run_on_startup: bool,
}

impl Settings {
    pub fn new(run_on_startup: bool) -> Self {
        Self {
            run_on_startup
        }
    }

    fn apply_to(&self, config: &mut Box<dyn Config>) {
        if let Err(e) = config.set_as_bool("RUN_ON_STARTUP", self.run_on_startup) {
            log::error!("cannot write config option {e}");
        }
    }
}

impl From<&Box<dyn Config>> for Settings{
    fn from(config: &Box<dyn Config>) -> Self {
        Self {
            run_on_startup: config.get_as_bool_or("RUN_ON_STARTUP", true)
        }
    }
}

pub struct AppCore {
    config: Box<dyn Config>,
    location_handler: Arc<Mutex<location::LocationHandler>>,
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<Credentials>,
    core_event_tx: tokio::sync::broadcast::Sender<CoreEvent>,
    core_cmd_tx: Sender<AppCoreCommand>,
    login_cancel_notify: Arc<Notify>,

    // filter state
    site: Option<String>,
    filter: PersonFilter,
}

#[derive(Clone)]
pub struct AppCoreRef {
    command_tx: tokio::sync::mpsc::Sender<AppCoreCommand>,
    event_tx: tokio::sync::broadcast::Sender<CoreEvent>,
}

#[derive(Debug, Clone)]
pub enum CoreEvent 
where Self: Send + Sync
{
    InitializationFinished,
    InitializationFailed,
    LoggingIn,
    LogginSuccessful,
    LoggedOut,
    SitesUpdated{sites: Vec<verishda_dto::types::Site>, selected_index: Option<usize>},
    PresencesChanged(Vec<verishda_dto::types::Presence>),
    Terminating,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum LoginPipeMessage {
    Cancel,
    HandleRedirect{
        code: String
    },
}

#[derive(Debug)]
enum AppCoreCommand {
    StartLogin,
    CancelCurrentOperation,
    ExchangeCodeForToken(String, PkceCodeVerifier),
    StartTokenRefresh,
    ReplaceCredentials(Credentials),
    Logout,
    RefreshPrecences,
    PublishAnnouncements{
        site_id: String,
        announcements: Vec<Announcement>
    },
    ChangeFavorite{
        user_id: String,
        favorite: bool
    },
    SetSite{
        site_id: String,
    },
    SetPersonFilter(PersonFilter),
    ApplySettings(Settings),
    Quit,
}

impl AppCore {
    pub fn new(config: Box<dyn Config>) -> AppCoreRef {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AppCoreCommand>(10);
        let (event_tx, _) = tokio::sync::broadcast::channel::<CoreEvent>(10);
        let core_ref = AppCoreRef {command_tx: tx.clone(), event_tx: event_tx.clone()};
        let mut app_core = Self {
            config,
            location_handler: location::LocationHandler::new(),
            oidc_metadata: None,
            oidc_client: None,
            credentials: None,
            core_event_tx: event_tx.clone(),
            core_cmd_tx: tx,
            site: None,
            login_cancel_notify: Arc::new(Notify::new()),
            filter: PersonFilter::default(),
        };

        // spawn AppCore event observer task, handling starting and stopping the
        // LocationHandler
        let location_handler = app_core.location_handler.clone();
        let mut event_rx = event_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                match event {
                    CoreEvent::LogginSuccessful => LocationHandler::start(location_handler.clone(), Duration::from_secs(5)).await,
                    CoreEvent::LoggingIn | CoreEvent::Terminating => LocationHandler::stop(location_handler.clone()).await,
                    _ => ()
                }
            }
        });

        // spawn AppCore background command handler task
        tokio::spawn(async move {

            log::info!("AppCore background task started");
            match app_core.init().await {
                Ok(_) => app_core.broadcast_core_event(CoreEvent::InitializationFinished).await,
                Err(e) => {
                    log::error!("initialization failed: {e}");
                    app_core.broadcast_core_event(CoreEvent::InitializationFailed).await
                },
            }

            // start with refreshing presences
            app_core.refresh_sites().await;

            // install interval timer
            let mut site_refresh_ival = tokio::time::interval(Duration::from_secs(5*60));
            site_refresh_ival.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut presence_refresh_ival = tokio::time::interval(Duration::from_secs(1*60));
            presence_refresh_ival.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            
            loop {
                tokio::select! {
                    _ = site_refresh_ival.tick() => {
                        app_core.refresh_sites().await;
                    }
                    _ = presence_refresh_ival.tick() => {
                        app_core.update_own_presence().await;
                        app_core.refresh_presences().await;
                    }
                    cmd = rx.recv() => {
                        if let Some(cmd) = cmd {
                            let quit = Self::process_command(&mut app_core, cmd).await;
                            if quit {
                                break;
                            }
                        }
                   }
                }
            }
        });


        core_ref
    }

    async fn process_command(app_core: &mut Self, cmd: AppCoreCommand) -> bool {
        use AppCoreCommand::*;
        match cmd {
            StartLogin => {
                AppCore::start_login(app_core).await.unwrap();
            }
            CancelCurrentOperation => {
                app_core.login_cancel_notify.notify_waiters();
            }
            ExchangeCodeForToken(code, pkce_verifier) => {
                if let Ok(()) = Self::exchange_code_for_tokens(app_core, code, pkce_verifier).await {
                    app_core.broadcast_core_event(CoreEvent::LogginSuccessful).await;
                }
            }
            ReplaceCredentials(credentials) => {
                app_core.credentials = Some(credentials);
                app_core.broadcast_core_event(CoreEvent::LogginSuccessful).await;
            }
            StartTokenRefresh => {
                if let Err(error) = Self::attempt_reconnect(app_core).await {
                    log::error!("unforeseen problem during token refresh: {error}");
                }
            },
            Logout => {
                app_core.credentials = None;
                app_core.broadcast_core_event(CoreEvent::LoggedOut).await;
            }
            RefreshPrecences => {
                app_core.update_own_presence().await;
                app_core.refresh_presences().await;
            },
            PublishAnnouncements{site_id, announcements} => {
                app_core.publish_own_announcements(site_id, announcements).await;
            },
            ChangeFavorite{user_id, favorite} => {
                app_core.publish_favorite_change(user_id, favorite).await;
            }
            Quit => {
                app_core.broadcast_core_event(CoreEvent::Terminating).await;
                return true;
            }
            SetPersonFilter(filter) => {
                app_core.set_filter(filter).await;
            }
            SetSite{site_id} => {
                app_core.set_site_impl(&site_id).await;
            }
            ApplySettings(settings) => {
                app_core.apply_settings_impl(settings).await;
            }
        }

        false
    }
}

impl AppCoreRef {

    fn send_cmd(&self, cmd: AppCoreCommand) {
        let cmd_str = format!("{cmd:?}");
        if let Err(e) = self.command_tx.blocking_send(cmd) {
            log::error!("failed to send command {cmd_str}");
        } else {
            log::trace!("command {cmd_str} sent");
        }
    }

    pub fn start_login(&self) {
        self.send_cmd(AppCoreCommand::StartLogin);
    }

    pub fn start_logout(&self) {
        self.send_cmd(AppCoreCommand::Logout);
    }

    pub fn set_site(&self, site_id: &str) {
        let site_id = site_id.to_owned();
        self.send_cmd(AppCoreCommand::SetSite{site_id});
    }

    pub fn filter(&self, filter: PersonFilter) {
        self.send_cmd(AppCoreCommand::SetPersonFilter(filter));
    }

    pub fn refresh(&self) {
        self.send_cmd(AppCoreCommand::RefreshPrecences);
    }

    pub fn change_favorite(&self, user_id: &str, favorite: bool) {
        let user_id = user_id.to_owned();
        self.send_cmd(AppCoreCommand::ChangeFavorite{user_id, favorite});
    }

    pub fn announce(&self, site_id: String, announcements: Vec<Announcement>) {
        self.send_cmd(AppCoreCommand::PublishAnnouncements{
            site_id, 
            announcements: announcements.clone()
        });
    }

    pub fn apply_settings(&self, settings: Settings) {
        self.send_cmd(AppCoreCommand::ApplySettings(settings));
    }

    pub fn quit(&self) {
        self.send_cmd(AppCoreCommand::Quit);
    }

    pub fn cancel_login(&self) {
        self.send_cmd(AppCoreCommand::CancelCurrentOperation);
    }

    pub fn on_core_event<F>(&self, f: F)
    where F: Fn(CoreEvent) + Send + 'static
    {
        let mut event_rx = self.event_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                f(event);
            }
        });
    }


}

impl AppCore {
    const RECONNECT_RETRY_INTERVAL: Duration = Duration::from_secs(10);

    async fn set_site_impl(&mut self, site_id: &str) {
        let new_site = if site_id.is_empty() {
           None
        } else {
           Some(site_id.to_string())
        };
        let changed = self.site != new_site;
        self.site = new_site;
        if changed {
            self.refresh_presences().await;
        }
    }

    async fn apply_settings_impl(&mut self, settings: Settings) {
        settings.apply_to(&mut (self.config));
    }

    async fn run_token_refresh(&mut self) -> Result<()> {
        let credentials;
        
        if let Some(c) = self.credentials.as_mut() {
            credentials = c;
        } else {
            return Err(anyhow::anyhow!("no refresh token available"));
        }

        let refresh_token = RefreshToken::new(credentials.refresh_token.clone());
        match self.oidc_client.as_ref().unwrap().exchange_refresh_token(&refresh_token)
            .request_async(async_http_client)
            .await 
        {
            Ok(resp) => {
                credentials.access_token = resp.access_token().secret().to_string();
                credentials.expires_at = Self::expires_at_from_now(resp.expires_in());
                return Ok(());
            }
            Err(e) => {
                self.credentials = None;
                self.broadcast_core_event(CoreEvent::LoggedOut).await;
                return Err(anyhow::anyhow!(e));
            }
        }
    }

    async fn create_client(&mut self) -> Result<verishda_dto::Client> {
        if let Some(credentials) = &self.credentials {
            if Instant::now().cmp(&credentials.expires_at) == std::cmp::Ordering::Greater{
                self.run_token_refresh().await?;
            }

            let mut headers = HeaderMap::new();
            let access_token = &self.credentials.as_ref().unwrap().access_token;
            headers.insert("Authorization", format!("Bearer {access_token}").parse().unwrap());
            let inner = reqwest::Client::builder()
                .default_headers(headers)
                .connection_verbose(true)
                .build()
                .expect("client creation failed");
            let client_inner = verishda_dto::ClientInner::new(self.core_cmd_tx.clone());
            let client = verishda_dto::Client::new_with_client(&self.api_base_url(), inner, client_inner);
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
                    drop(location_handler);

                    // find out new selected site_id and index after
                    // filtering the current selection against the
                    // sites list we just received
                    let site_index = self.site.as_ref()
                    .or(sites.get(0).map(|s|&s.id))
                    .and_then(|selected_id|{
                        sites.iter()
                        .position(|site|&site.id == selected_id)
                        .map(|i|(selected_id.clone(),i))
                    });

                    let selected_index;
                    match site_index {
                        Some((site_id, index)) => {
                            selected_index = Some(index);
                            self.site = Some(site_id);
                        }
                        None => {
                            selected_index = None;
                            self.site = None;
                        }
                    }
                    self.broadcast_core_event(CoreEvent::SitesUpdated{sites, selected_index}).await;

                    self.refresh_presences().await;
                }
                Err(e) => {
                    log::error!("Failed to get sites: {}", e);
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

        let client = match self.create_client().await {
            Ok(client) => client,
            Err(error) => {
                log::error!("failed to create client: {error}");
                return;
            }
        };

        log::trace!("Refreshing presences");
        let site = if let Some(site) = &self.site {
            site
        } else {
            log::trace!("No site selected, aborting presence refresh");
            return;
        };

        log::trace!("Getting presences for site {site}");
        let term = self.filter.term.as_ref()
            .filter(|t|!t.is_empty())
            .map(|t|t.as_str());
        let favorites_only = Some(self.filter.favorites_only);
        match client.handle_get_sites_siteid_presence(site, favorites_only, None, None, term).await {
            Ok(sites_response) => {
                let presences = sites_response.into_inner();
                log::debug!("Got presences: {:?}", presences);
                self.broadcast_core_event(CoreEvent::PresencesChanged(presences)).await;
            }
            Err(e) => {
                log::error!("Failed to get sites: {}", e);
            }
        }
    }

    async fn set_filter(&mut self, filter: PersonFilter) {
        self.filter = filter;
        self.refresh_presences().await;
    }

    async fn apply_settings(&mut self, settings: Settings) {
        settings.apply_to(&mut self.config);
    }

    async fn publish_favorite_change(&mut self, user_id: String, favorite: bool) {
        let client: verishda_dto::Client = match self.create_client().await {
            Err(e) => {
                log::error!("can't create client: {e}");
                return
            }
            Ok(c) => c,
        };
        let call_result = if favorite {
            client.handle_put_favorite(&user_id).await
        } else {
            client.handle_delete_favorite(&user_id).await
        };

        if let Err(e) = call_result {
            log::error!("call to set favorite status failed: {e}");
        }

        self.refresh_presences().await;
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

    async fn broadcast_core_event(&self, event: CoreEvent) {
        self.core_event_tx.send(event).unwrap_or_else(|e|{
            log::error!("failed to send core event {e}");
            return 0
        });
    }

    fn api_base_url(&self) -> String{
        self.config.get("API_BASE_URL").unwrap()
    }

    fn redirect_url(&self) -> String {
        self.api_base_url() + "/api/public/oidc/login-target"
    }

    async fn start_login(app_core: &mut AppCore) -> Result<()> 
    {
        let shutdown_notify;
        {
            app_core.broadcast_core_event(CoreEvent::LoggingIn).await;
            shutdown_notify = app_core.login_cancel_notify.clone();
        }
        let url = Self::start_login_websocket(app_core, shutdown_notify.clone())?;

        if let Err(e) = webbrowser::open(&url.to_string()) {
            log::error!("Failed to open URL: {}", e);
        }
        Ok(())
    }

    async fn attempt_reconnect(app_core: &mut AppCore) -> Result<()> {
        // FIXME: need to shut down location manager
        if let Some(credentials) = &app_core.credentials {
            app_core.broadcast_core_event(CoreEvent::LoggingIn).await;
            let refresh_token = RefreshToken::new(credentials.refresh_token.clone());
            let oidc_client = app_core.oidc_client.as_ref().unwrap().clone();
            let cmd_tx = app_core.core_cmd_tx.clone();

            let shutdown_notify = app_core.login_cancel_notify.clone();

            tokio::spawn(async move {

                // set retry intverval so that:
                // we retry connecting every couple of seconds
                // we skip missed ticks in case program was paused, either by the harware
                // (laptop) going to sleep, or program begin suspeded in the debugger.
                let mut retry_interval = tokio::time::interval(Self::RECONNECT_RETRY_INTERVAL);
                retry_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

                loop {
                    log::debug!("attempting token refresh");

                    let refresh_result = oidc_client
                    .exchange_refresh_token(&refresh_token)
                    .request_async(async_http_client).await;
                
                    match refresh_result {
                        Ok(token_response) => {
                            let r = refresh_token.secret().clone();
                            let c = Self::credentials_from_token_response_now(&token_response, Some(r));
                            cmd_tx.send(AppCoreCommand::ReplaceCredentials(c)).await.unwrap();
                            log::debug!("token refresh succeeded");
                            break;
                        }
                        Err(error) =>  {
                            let refresh_token_invalid;
                            match error {
                                openidconnect::RequestTokenError::ServerResponse(server_response) => {
                                    match server_response.error() {
                                        openidconnect::core::CoreErrorResponseType::InvalidGrant => {
                                            refresh_token_invalid = true
                                        }
                                        _ => refresh_token_invalid = false
                                    }
                                }
                                _ => refresh_token_invalid = false
                            };

                            if refresh_token_invalid {
                                // abort retry
                                log::debug!("token refresh failed");
                                cmd_tx.send(AppCoreCommand::Logout).await.unwrap();
                                break;
                            } else {
                                log::debug!("error while token refresh, retrying...");
                                tokio::select! {
                                    _ = shutdown_notify.notified() => {
                                        cmd_tx.send(AppCoreCommand::Logout).await.unwrap();
                                        break
                                    }
                                    _ = retry_interval.tick() => continue,
                                }
                            }
                        }   
                    }             
                };
            });
        } else {
            return Err(anyhow::anyhow!("no credentials"))
        }
        Ok(())
    }

    fn start_login_websocket(app_core: &mut AppCore, shutdown_notify: Arc<Notify>) -> Result<Url> {
        let (auth_url, pkce_verifier, csrf_token) = {
            app_core.authorization_url()
        };

        let baseurl = app_core.api_base_url();
        let ws_url = baseurl + "/api/public/oidc/login-requests/" + csrf_token.secret();
        let mut ws_url = Url::parse(&ws_url).unwrap();
        match ws_url.scheme() {
            "http" => ws_url.set_scheme("ws").unwrap(),
            "https" => ws_url.set_scheme("wss").unwrap(),
            _ => panic!("unsupported scheme")
        };

        let cmd_tx = app_core.core_cmd_tx.clone();

        tokio::spawn( async move {
            let (mut ws_stream, _) = match tokio_tungstenite::connect_async(&ws_url).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("failed to connect to code receving websocket service on url {ws_url} with error '{e}'");
                    return
                }
            };

            let mut cmd = AppCoreCommand::Logout;
            tokio::select! {
                _ = shutdown_notify.notified() => {
                    return;
                }
                ws_result = ws_stream.next() => match ws_result {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(code))) => {
                        cmd = AppCoreCommand::ExchangeCodeForToken(code, pkce_verifier);
                    }
                    Some(Ok(msg)) => {
                        log::error!("wrong message type received: {msg}");
                    }
                    Some(Err(e)) => {
                        log::error!("error while reading from websocket: {e}");
                    }
                    None => {
                        log::error!("stream terminated without providing code");
                    }
                }
            }
            cmd_tx.send(cmd).await.unwrap();
        });
        Ok(auth_url)
    }

    fn credentials_from_token_response_now<EF,TT>(token_response: &StandardTokenResponse<EF,TT>, fallback_refresh_token: Option<String>)
    -> Credentials
    where 
    EF: ExtraTokenFields,
    TT: TokenType,
    {
        let refresh_token = token_response
        .refresh_token()
        .map(|r|r.secret().clone())
        .or(fallback_refresh_token)
        .expect("either a refresh_token must be present in the response, or a fallback token must be given");

        Credentials {
            access_token: token_response.access_token().secret().clone(),
            refresh_token,
            expires_at: Self::expires_at_from_now(token_response.expires_in())
        }
    }

    fn expires_at_from_now(expires_in: Option<Duration>) -> Instant {
        let expires_in = expires_in
        .unwrap_or(Duration::from_secs(60));

        // reduce the expiration time by 10% to account for clock skew
        let expires_in = expires_in * 9 / 10;

        Instant::now() + expires_in
    }

    async fn exchange_code_for_tokens(app_core: &mut AppCore, code: String, pkce_verifier: PkceCodeVerifier) -> Result<()> {
        let client = app_core.oidc_client.as_ref().unwrap();
        let token_response = client.exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await?;
        let credentials = Self::credentials_from_token_response_now(&token_response, None);

        log::info!("Exchanged into access_token {credentials:?}");
        app_core.credentials = Some(credentials);
        app_core.refresh_sites().await;

        Ok(())
    }

    fn authorization_url(&self) -> (Url, PkceCodeVerifier, CsrfToken) {
        // Generate a PKCE challenge.
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Generate the full authorization URL.
        let (auth_url, csrf_token, _nonce) = self.oidc_client.as_ref().unwrap()
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            // Set the PKCE code challenge.
            .set_pkce_challenge(pkce_challenge)
            .add_scope(Scope::new("offline_access".into()))
            .url();

        (auth_url, pkce_verifier, csrf_token)
    }

    async fn init(&mut self) -> Result<()>{
        let issuer_url = self.config.get("ISSUER_URL")?;
        let client_id = self.config.get("CLIENT_ID")?;
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