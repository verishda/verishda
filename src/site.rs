use anyhow::Result;
use serde::{Serialize, Deserialize};
use spin_sdk::{pg::{ParameterValue, Decode, self}, config};
use chrono::NaiveDate;

#[derive(Serialize)]
pub(super) struct Site {
    id: String,
    name: String,
    longitude: f32,
    latitude: f32
}

#[derive(Serialize)]
pub(super) struct Presence {
    pub logged_as_name: String,
//    pub last_seen: i64,
}

#[derive(Deserialize)]
pub(super) struct PresenceAnnouncement {
    date: NaiveDate,
    site_id: String,
}


pub(super) fn get_sites() -> Result<Vec<Site>> {
    let stmt = String::new() +
    "SELECT id, name, longitude, latitude FROM sites";

    let row_set = pg::query(&pg_address()?, &stmt, &[])?;
    let sites: Vec<_> = row_set
    .rows.iter()
    .map(|r|Site {
        id: String::decode(&r[0]).unwrap(),
        name: String::decode(&r[1]).unwrap(), 
        longitude: f32::decode(&r[2]).unwrap(), 
        latitude: f32::decode(&r[3]).unwrap(),
    })
    .collect()
    ;

    Ok(sites)
}


fn pg_address() -> Result<String> {
    Ok(config::get("pg_address")?)
}

pub(super) fn hello_site(user_id: &str, logged_as_name: &str, site_id: &str) -> Result<()>{
    let stmt = String::new() +
    "INSERT INTO logged_into_site (user_id, logged_as_name, site_id, last_seen) VALUES ($1, $2, $3, now()) ON CONFLICT (user_id) 
    DO UPDATE SET logged_as_name=$2, site_id=$3, last_seen=now()";

    let params = [
        ParameterValue::Str(user_id),
        ParameterValue::Str(logged_as_name),
        ParameterValue::Str(site_id),
    ];
    pg::execute(&pg_address()?, &stmt, &params).unwrap();

    Ok(())
}

pub(super) fn get_presence_on_site(site_id: &str) -> Result<Vec<Presence>> {
    let stmt = String::new() +
    "SELECT logged_as_name, last_seen FROM logged_into_site WHERE site_id=$1 AND last_seen >= now() - INTERVAL '10 minutes'";

    let row_set = pg::query(&pg_address()?, &stmt, &[
        ParameterValue::Str(site_id),
    ])?;
    let presences: Vec<_> = row_set
    .rows.iter()
    .map(|r|Presence {
        logged_as_name: String::decode(&r[0]).unwrap(), 
//        last_seen: i64::decode(&r[1]).unwrap(), 
    })
    .collect()
    ;
    
    Ok(presences)
}

pub(super) fn announce_presence_on_site(user_id: &str, announcements: &[PresenceAnnouncement]) -> Result<()> {

    let mut announcements_str = String::from("ARRAY [");
    if !announcements.is_empty() {
        announcements_str += announcements.iter()
            .map(|a|{
                let sql_date = a.date.format("%Y/%m/%d").to_string();
                format!("ROW('{}','{}')::announcement_t", a.site_id, sql_date)
            })
            .collect::<Vec<_>>()
            .join(", ")
            .as_str()
            ;
    }
    announcements_str += "]";

    let params = [];
    let stmt = format!(
        "INSERT INTO user_announcements (user_id, announcements) VALUES ('{0}', {1}) ON CONFLICT (user_id) 
        DO UPDATE SET announcements={1}",
        user_id,
        announcements_str
    );

    println!("stmt: {}", stmt);

    pg::execute(&pg_address()?, &stmt, &params).unwrap();

    Ok(())

}