
use dotenv::dotenv;
use sqlx::postgres::PgPoolOptions;
use anyhow::*;
use verishda::config::Config;

fn init_dotenv() {
    if let Result::Ok(path) = dotenv() {
        let path = path.to_string_lossy();
        println!("additional environment variables loaded from {path}");
    }
}

#[derive(Clone)]
struct EnvConfig;

impl Config for EnvConfig{
    fn get(&self, key: &str) -> Result<String> {
        std::env::var(key).map_err(|_| anyhow!("no such environment variable {key}"))
    }
    fn clone_box_dyn(&self) -> Box<dyn Config> {
        Box::new(self.clone())
    }
}

#[tokio::main]
async fn main(){
    let executable_name = std::env::args().next().unwrap_or_else(||"unknown".to_string());
    println!("starting {executable_name}...");

    init_dotenv();


    let config = EnvConfig;
    verishda::init_logging(config.clone());

    log::debug!("connecting to database...");
    let pg_address = std::env::var("PG_ADDRESS")
    .expect("no postgres database connection configured, set PG_ADDRESS variable");
    let pool = PgPoolOptions::new()
        .connect(&pg_address).await.expect(&format!("could not connect to database {pg_address}"));
    
    let router = verishda::build_router(pool, config.clone());
    
    let bind_address = std::env::var("BIND_ADDRESS")
    .unwrap_or_else(|_|"127.0.0.1:3000".to_string());

    let listener = tokio::net::TcpListener::bind(&bind_address).await.unwrap();
    log::info!("binding, server available under http://{bind_address}");
    axum::serve(listener, router.into_make_service())
    .await
    .unwrap();
}
