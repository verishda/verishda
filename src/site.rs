use std::collections::HashMap;

use anyhow::Result;
use serde::{Serialize, Deserialize};
use chrono::NaiveDate;


use crate::PgConnection;

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
    pub currently_present: bool,
    pub announced_dates: Vec<NaiveDate>,
}

#[derive(Deserialize)]
pub(super) struct PresenceAnnouncement {
    date: NaiveDate,
}


pub(super) async fn get_sites(pg: PgConnection) -> Result<Vec<Site>> 
where Result<Vec<Site>>: Send + Sync
{

    let stmt = String::new() +
    "SELECT id, name, longitude, latitude FROM sites";

    let row_set = pg.query(&stmt, &[]).await?;
    let sites: Vec<_> = row_set
    .iter()
    .map(|r|Site {
        id: r.get(0),
        name: r.get(1), 
        longitude: r.get(2), 
        latitude: r.get(3),
    })
    .collect()
    ;

    Ok(sites)
}


pub(super) async fn hello_site(pg: &PgConnection, user_id: &str, logged_as_name: &str, site_id: &str) -> Result<()>{

    update_userinfo(pg, user_id, logged_as_name).await?;

    let stmt = String::new() +
    "INSERT INTO logged_into_site (user_id, logged_as_name, site_id, last_seen) VALUES ($1, $2, $3, now()) ON CONFLICT (user_id) 
    DO UPDATE SET logged_as_name=$2, site_id=$3, last_seen=now()";

    pg.execute(&stmt, &[
        &user_id.to_string(),
        &logged_as_name.to_string(),
        &site_id.to_string(),
    ]).await?;

    Ok(())
}

pub(super) async fn get_presence_on_site(pg: PgConnection, site_id: &str) -> Result<Vec<Presence>> {

    let stmt = String::new() +
    "SELECT u.user_id, u.logged_as_name, to_char(a.present_on, 'YYYY-MM-DD')
    FROM user_announcements AS a JOIN user_info AS u ON a.user_id=u.user_id 
    WHERE a.site_id=$1
    
    UNION
    
    SELECT u.user_id, u.logged_as_name, NULL 
    FROM logged_into_site AS s JOIN user_info AS u ON s.user_id=u.user_id 
    WHERE s.site_id=$1 AND u.last_seen >= now() - INTERVAL '10 minutes'";

    let row_set = pg.query(&stmt, &[
        &site_id.to_string(),
    ]).await?;

    let presences: Vec<_> = row_set
    .iter()
    .fold(HashMap::<String,Presence>::new(), |mut m,r|{
        let user_id = r.get::<_,String>(0);
        let present_on = if let Some(s) = r.get::<_,Option<String>>(2) {
            let date = NaiveDate::parse_from_str(&s, "%Y-%m-%d").unwrap();
            Some(date)
        } else {
            None
        };

        if !m.contains_key(&user_id) {
            m.insert(user_id.clone(), Presence {
                logged_as_name: r.get(1), 
                ..Default::default()}
            );
        }
        let presence = m.get_mut(&user_id).unwrap();

        match present_on {
            // having a presence announcement date means exactly that: an announcement 
            Some(date) => presence.announced_dates.push(date),
            // having none means that user is currently present
            None => presence.currently_present = true,
        }

        m
    })
    .values()
    .map(|p|p.clone())
    .collect()
    ;

    Ok(presences)
}


async fn update_userinfo(pg: &PgConnection, user_id: &str, logged_as_name: &str) -> Result<()> {
    
    let stmt = "INSERT INTO user_info (user_id, logged_as_name, last_seen) VALUES ($1, $2, now()) ON CONFLICT (user_id) 
    DO UPDATE SET logged_as_name=$2, last_seen=now()";
    pg.execute(stmt, &[
        &user_id.to_string(),
        &logged_as_name.to_string()
        ]
    ).await?;
    
    Ok(())
}

pub(super) async fn announce_presence_on_site(pg: &mut PgConnection, user_id: &str, site_id: &str, logged_as_name: &str, announcements: &[PresenceAnnouncement]) -> Result<()> {

    update_userinfo(pg, user_id, logged_as_name).await?;

    let pg = pg.transaction().await?;
    pg.execute("DELETE FROM user_announcements WHERE user_id=$1 AND site_id=$2", &[
        &user_id.to_string(),
        &site_id.to_string()
    ]).await?;

    for a in announcements {
        let sql_date = a.date.format("%Y/%m/%d").to_string();
        
        let stmt = format!("INSERT INTO user_announcements (user_id, site_id, present_on) VALUES ($1, $2, '{}')", sql_date);
        pg.execute(&stmt, &[
            &user_id.to_string(),
            &site_id.to_string()
        ]).await?;
    }
    Ok(pg.commit().await?)
}