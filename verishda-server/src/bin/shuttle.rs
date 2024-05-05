use shuttle_axum::ShuttleAxum;
use sqlx::PgPool;
use shuttle_runtime::SecretStore;
use verishda::config::Config;
use anyhow::{Result, anyhow};

#[derive(Clone)]
struct ShuttleConfig {
    secret_store: SecretStore
}

impl Config for ShuttleConfig{
    fn get(&self, key: &str) -> Result<String> {
        self.secret_store.get(key).ok_or_else(||anyhow!("config key {key} not found"))
    }
    fn clone_box_dyn(&self) -> Box<dyn Config> {
        Box::new(self.clone())
    }
}

#[shuttle_runtime::main]
async fn axum(
    #[shuttle_shared_db::Postgres] pool: PgPool, 
    #[shuttle_runtime::Secrets] secret_store: SecretStore
) -> ShuttleAxum {

    let config = ShuttleConfig {secret_store};

    Ok(verishda::build_router(pool, config).into())
}
