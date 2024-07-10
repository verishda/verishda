use sqlx::prelude::FromRow;


#[derive(FromRow)]
struct UserInfo {
    user_id: String,
    logged_as_name: String,
    last_seen: std::time::Instant,
}
