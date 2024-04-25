use std::collections::HashMap;

use anyhow::Result;
use serde::{Serialize, Deserialize};
use chrono::NaiveDate;
use sqlx::{Connection, Postgres, PgConnection, postgres::PgRow, Row};


#[derive(Serialize)]
pub(super) struct Site 
where Self: Send
{
    id: String,
    name: String,
    longitude: f32,
    latitude: f32
}

#[derive(Serialize, Default, Clone)]
pub(super) struct Presence 
where Self: Send
{
    pub logged_as_name: String,
    pub is_self: bool,
    pub currently_present: bool,
    pub announced_dates: Vec<NaiveDate>,
}

#[derive(Deserialize)]
pub(super) struct PresenceAnnouncement {
    date: NaiveDate,
}


pub(super) async fn get_sites(pg: &mut PgConnection) -> Result<Vec<Site>> 
where Result<Vec<Site>>: Send + Sync
{

    let sites = sqlx::query("SELECT id, name, longitude, latitude FROM sites")
    .map(|r: PgRow|Site {
        id: r.get(0),
        name: r.get(1), 
        longitude: r.get(2), 
        latitude: r.get(3),
    })
    .fetch_all(pg).await?
    ;

    Ok(sites)
}


pub(super) async fn hello_site(pg: &mut PgConnection, user_id: &str, logged_as_name: &str, site_id: &str) -> Result<()>{

    update_userinfo(pg, user_id, logged_as_name).await?;

    let stmt = String::new() +
    "INSERT INTO logged_into_site (user_id, logged_as_name, site_id, last_seen) VALUES ($1, $2, $3, now()) ON CONFLICT (user_id) 
    DO UPDATE SET logged_as_name=$2, site_id=$3, last_seen=now()";

    sqlx::query(&stmt)
    .bind(&user_id.to_string())
    .bind(&logged_as_name.to_string())
    .bind(&site_id.to_string())
    .execute(pg)
    .await?;

    Ok(())
}

pub(super) async fn get_presence_on_site(pg: &mut PgConnection, user_id: &str, logged_as_name: &str, site_id: &str) -> Result<Vec<Presence>> {

    let stmt = String::new() +
    "SELECT u.user_id, u.logged_as_name, to_char(a.present_on, 'YYYY-MM-DD')
    FROM user_announcements AS a JOIN user_info AS u ON a.user_id=u.user_id 
    WHERE a.site_id=$1
    
    UNION
    
    SELECT u.user_id, u.logged_as_name, NULL 
    FROM logged_into_site AS s JOIN user_info AS u ON s.user_id=u.user_id 
    WHERE s.site_id=$1 AND u.last_seen >= now() - INTERVAL '10 minutes'";

    let mut presences_map = sqlx::query(&stmt)
    .bind(&site_id.to_string())
    .fetch_all(pg).await?
    .iter()
    .fold(HashMap::<String,Presence>::new(), |mut m,r|{
        let user_id: &str = r.get(0);
        let present_on = if let Some(s) = r.get(2) {
            let date = NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
            Some(date)
        } else {
            None
        };

        if !m.contains_key(user_id) {
            m.insert(user_id.to_string(), Presence {
                logged_as_name: r.get(1), 
                ..Default::default()}
            );
        }
        let presence = m.get_mut(user_id).unwrap();

        match present_on {
            // having a presence announcement date means exactly that: an announcement 
            Some(date) => presence.announced_dates.push(date),
            // having none means that user is currently present
            None => presence.currently_present = true,
        }

        m
    });

    let mut self_presence = match presences_map.remove(user_id) {
        Some(p) => p,
        None => Presence{
            currently_present: false,
            logged_as_name: logged_as_name.to_string(),
            announced_dates: Vec::new(),
            ..Default::default()
        },
    };
    self_presence.is_self = true;

    let presences = std::iter::once(self_presence)
    .chain(
        presences_map.values()
        .map(Presence::clone)
    )
    .collect();

    Ok(presences)
}


async fn update_userinfo(pg: &mut PgConnection, user_id: &str, logged_as_name: &str) -> Result<()> {
    
    let stmt = "INSERT INTO user_info (user_id, logged_as_name, last_seen) VALUES ($1, $2, now()) ON CONFLICT (user_id) 
    DO UPDATE SET logged_as_name=$2, last_seen=now()";
    
    sqlx::query(stmt)
    .bind(&user_id.to_string())
    .bind(&logged_as_name.to_string())
    .execute(pg).await?;
    
    Ok(())
}

pub(super) async fn announce_presence_on_site(pg: &mut PgConnection, user_id: &str, site_id: &str, logged_as_name: &str, announcements: &[PresenceAnnouncement]) -> Result<()> {

    update_userinfo(pg, user_id, logged_as_name).await?;

    let mut tr: sqlx::Transaction<'_, Postgres> = pg.begin().await?;
    sqlx::query("DELETE FROM user_announcements WHERE user_id=$1 AND site_id=$2")
        .bind(&user_id.to_string())
        .bind(&site_id.to_string())
        .execute(&mut *tr)
        .await?;

    for a in announcements {
        let sql_date = a.date.format("%Y/%m/%d").to_string();
        
        let stmt = format!("INSERT INTO user_announcements (user_id, site_id, present_on) VALUES ($1, $2, '{}')", sql_date);
        sqlx::query(&stmt)
        .bind(&user_id.to_string())
        .bind(&site_id.to_string())
        .execute(&mut *tr)
        .await?;
    }
    Ok(tr.commit().await?)
}