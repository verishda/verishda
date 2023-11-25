use shuttle_axum::ShuttleAxum;
use sqlx::PgPool;


#[shuttle_runtime::main]
async fn axum(#[shuttle_shared_db::Postgres] pool: PgPool) -> ShuttleAxum {
    Ok(verishda::build_router(pool).into())
}
