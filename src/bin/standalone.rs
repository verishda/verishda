
use dotenv::dotenv;
use sqlx::postgres::PgPoolOptions;
use tokio::main;

fn init_dotenv() {
    if let Ok(path) = dotenv() {
        let path = path.to_string_lossy();
        println!("additional environment variables loaded from {path}");
    }
}



#[tokio::main]
async fn main(){
    let executable_name = std::env::args().next().unwrap_or_else(||"unknown".to_string());
    println!("starting {executable_name}...");

    init_dotenv();
    verishda::init_logging();

    log::debug!("connecting to database...");
    let pg_address = verishda::config::get("pg_address")
    .expect("no postgres database connection configured, set PG_ADDRESS variable");
    let pool = PgPoolOptions::new()
        .connect(&pg_address).await.expect(&format!("could not connect to database {pg_address}"));
    
    let router = verishda::build_router(pool);
    
    let bind_address = verishda::config::get("bind_address")
    .unwrap_or_else(|_|"127.0.0.1:3000".to_string())
    .parse()
    .unwrap();

    log::info!("binding, server available under http://{bind_address}");
    axum::Server::bind(&bind_address)
    .serve(router.into_make_service())
    .await
    .unwrap();
}
