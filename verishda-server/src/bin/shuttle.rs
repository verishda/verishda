use shuttle_axum::ShuttleAxum;
use sqlx::PgPool;
use shuttle_runtime::SecretStore;
use verishda_config::{default_config, CompositeConfig, Config};
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
    #[shuttle_shared_db::Postgres] pg_url: String, 
    #[shuttle_runtime::Secrets] secret_store: SecretStore
) -> ShuttleAxum {

    let shuttle_config = ShuttleConfig {secret_store};
    let config = CompositeConfig::from_configs(
        Box::new(shuttle_config), 
        Box::new(default_config())
    );

    let pool = verishda::connect_db(&pg_url).await?;
    Ok(verishda::build_router(pool, config).into())
}
