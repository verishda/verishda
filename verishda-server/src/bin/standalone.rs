
use anyhow::*;
use verishda_config::{default_config, CompositeConfig, EnvConfig};


#[tokio::main]
async fn main(){
    let executable_name = std::env::args().next().unwrap_or_else(||"unknown".to_string());
    println!("starting {executable_name}...");

    let config = CompositeConfig::from_configs(
        Box::new(EnvConfig::from_env()),
        Box::new(default_config())
    );
    verishda::init_logging(&config);

    log::debug!("connecting to database...");
    let pg_address = std::env::var("PG_ADDRESS")
    .expect("no postgres database connection configured, set PG_ADDRESS variable");
    let pool = verishda::connect_db(&pg_address).await.expect(&format!("could not connect to database {pg_address}"));
    log::debug!("connected.");
    
    let router = verishda::build_router(pool, config.clone());
    
    let bind_address = std::env::var("BIND_ADDRESS")
    .unwrap_or_else(|_|"127.0.0.1:3000".to_string());

    let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
    log::info!("binding, server available under http://{bind_address}");
    axum::serve(listener, router.into_make_service())
    .await
    .unwrap();
}
