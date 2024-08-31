use std::{sync::Arc, time::{Duration, Instant}};

use chrono::Days;
use futures::prelude::*;
use openidconnect::{core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata}, reqwest::async_http_client, AuthorizationCode, ClientId, CsrfToken, IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken};
use anyhow::Result;

use reqwest::header::HeaderMap;
use tokio::sync::{Mutex, Notify};
use url::Url;
use log::*;

use verishda_config::Config;
use verishda_dto::{self, types::{PresenceAnnouncement, PresenceAnnouncementKind, PresenceAnnouncements}};
use crate::core::location::Location;

mod location;

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

#[derive(Default)]
pub struct PersonFilter {
    pub favorites_only: bool,
    pub term: Option<String>,
}


pub struct AppCore {
    config: Box<dyn Config>,
    location_handler: Arc<Mutex<location::LocationHandler>>,
    oidc_metadata: Option<CoreProviderMetadata>,
    oidc_client: Option<CoreClient>,
    credentials: Option<Credentials>,
    command_tx: tokio::sync::mpsc::Sender<AppCoreCommand>,
    on_core_event: Option<Box<dyn Fn(CoreEvent) + Send>>,
    login_cancel_notify: Arc<Notify>,

    // filter state
    site: Option<String>,
    filter: PersonFilter,
}

#[derive(Debug)]
pub enum CoreEvent {
    LoggingIn,
    LogginSuccessful,
    LoggedOut,
    SitesUpdated{sites: Vec<verishda_dto::types::Site>, selected_index: Option<usize>},
    PresencesChanged(Vec<verishda_dto::types::Presence>),
}

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
    ChangeFavorite{
        user_id: String,
        favorite: bool
    },
    SetSite{
        site_id: String,
    },
    SetPersonFilter(PersonFilter),
    Quit,
}

impl AppCore {
    pub fn new(config: Box<dyn Config>) -> Arc<Mutex<Self>> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AppCoreCommand>(1);
        let app_core = Self {
            config,
            location_handler: location::LocationHandler::new(),
            command_tx: tx,
            oidc_metadata: None,
            oidc_client: None,
            credentials: None,
            on_core_event: None,
            site: None,
            login_cancel_notify: Arc::new(Notify::new()),
            filter: PersonFilter::default(),
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
                            use AppCoreCommand::*;
                            match cmd {
                                RefreshPrecences => {
                                    app_core_clone.lock().await.update_own_presence().await;
                                    app_core_clone.lock().await.refresh_presences().await;
                                },
                                PublishAnnouncements{site_id, announcements} => {
                                    app_core_clone.lock().await.publish_own_announcements(site_id, announcements).await;
                                },
                                ChangeFavorite{user_id, favorite} => {
                                    app_core_clone.lock().await.publish_favorite_change(user_id, favorite).await;
                                }
                                Quit => {
                                    break;
                                }
                                SetPersonFilter(filter) => {
                                    app_core_clone.lock().await.set_filter(filter).await;
                                }
                                SetSite{site_id} => {
                                    app_core_clone.lock().await.set_site_impl(&site_id).await;
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
        let site_id = site_id.to_owned();
        _ = self.command_tx.blocking_send(AppCoreCommand::SetSite{site_id});
    }

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

    pub fn filter(&mut self, filter: PersonFilter) {
        _ = self.command_tx.blocking_send(AppCoreCommand::SetPersonFilter(filter));
    }

    pub fn refresh(&self) {
        _ = self.command_tx.blocking_send(AppCoreCommand::RefreshPrecences);
    }

    pub fn change_favorite(&self, user_id: &str, favorite: bool) {
        let user_id = user_id.to_owned();
        _ = self.command_tx.blocking_send(AppCoreCommand::ChangeFavorite{user_id, favorite});
    }

    pub fn announce(&self, site_id: String, announcements: Vec<Announcement>) {
        _ = self.command_tx.blocking_send(AppCoreCommand::PublishAnnouncements{
            site_id, 
            announcements: announcements.clone()
        });
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
            let client = verishda_dto::Client::new_with_client(&self.api_base_url(), inner);
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
                    self.broadcast_core_event(CoreEvent::SitesUpdated{sites, selected_index});

                    self.refresh_presences().await;
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
            let term = self.filter.term.as_ref()
                .filter(|t|!t.is_empty())
                .map(|t|t.as_str());
            let favorites_only = Some(self.filter.favorites_only);
            match client.handle_get_sites_siteid_presence(site, favorites_only, None, None, term).await {
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

    async fn set_filter(&mut self, filter: PersonFilter) {
        self.filter = filter;
        self.refresh_presences().await;
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
    fn api_base_url(&self) -> String{
        self.config.get("API_BASE_URL").unwrap()
    }

    fn redirect_url(&self) -> String {
        self.api_base_url() + "/api/public/oidc/login-target"
    }

    pub fn start_login(app_core: Arc<Mutex<AppCore>>) -> Result<()> 
    {
        let shutdown_notify;
        {
            let app_core = app_core.blocking_lock();
            app_core.broadcast_core_event(CoreEvent::LoggingIn);
            shutdown_notify = app_core.login_cancel_notify.clone();
        }
        let url = Self::start_login_websocket(app_core.clone(), shutdown_notify.clone())?;

        if let Err(e) = webbrowser::open(&url.to_string()) {
            log::error!("Failed to open URL: {}", e);
        }
        Ok(())
    }

    fn start_login_websocket(app_core: Arc<Mutex<AppCore>>, shutdown_notify: Arc<Notify>) -> Result<Url> {
        let app_core_clone = app_core.clone();
        let (auth_url, pkce_verifier, csrf_token) = {
            app_core.clone().blocking_lock().authorization_url()
        };
        let app_core = app_core.clone();

        let baseurl = app_core.blocking_lock().api_base_url();
        let ws_url = baseurl + "/api/public/oidc/login-requests/" + csrf_token.secret();
        let mut ws_url = Url::parse(&ws_url).unwrap();
        match ws_url.scheme() {
            "http" => ws_url.set_scheme("ws").unwrap(),
            "https" => ws_url.set_scheme("wss").unwrap(),
            _ => panic!("unsupported scheme")
        };
        tokio::spawn( async move {
            let (mut ws_stream, _) = match tokio_tungstenite::connect_async(&ws_url).await {
                Ok(s) => s,
                Err(e) => {
                    log::error!("failed to connect to code receving websocket service on url {ws_url} with error '{e}'");
                    return
                }
            };

            tokio::select! {
                _ = shutdown_notify.notified() => {
                    return;
                }
                ws_result = ws_stream.next() => match ws_result {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(code))) => {
                        if let Ok(()) = Self::exchange_code_for_tokens(app_core, code, pkce_verifier).await {
                            app_core_clone.lock().await.broadcast_core_event(CoreEvent::LogginSuccessful);
                            return;
                        }
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
            app_core_clone.lock().await.broadcast_core_event(CoreEvent::LoggedOut);
        });
        Ok(auth_url)
    }

    pub fn cancel_login(&mut self) {
        self.login_cancel_notify.notify_waiters();
    }

    fn expires_at_from_now(expires_in: Option<Duration>) -> Instant {
        let expires_in = expires_in
        .unwrap_or(Duration::from_secs(60));

        // reduce the expiration time by 10% to account for clock skew
        let expires_in = expires_in * 9 / 10;

        Instant::now() + expires_in
    }

    async fn exchange_code_for_tokens(app_core: Arc<Mutex<AppCore>>, code: String, pkce_verifier: PkceCodeVerifier) -> Result<()> {
        let mut app_core = app_core.lock().await;
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
            .url();

        (auth_url, pkce_verifier, csrf_token)
    }

    pub async fn init(&mut self) -> Result<()>{
        let issuer_url = self.config.get("ISSUER_URL")?;
        let client_id = self.config.get("CLIENT_ID")?;
        self.init_provider(&issuer_url, &client_id).await?;
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