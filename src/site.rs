use std::{panic::UnwindSafe, collections::HashMap};

use anyhow::Result;
use serde::{Serialize, Deserialize};
use spin_sdk::{pg::{ParameterValue, Decode, self, DbValue}, config};
use chrono::NaiveDate;

#[derive(Serialize)]
pub(super) struct Site {
    id: String,
    name: String,
    longitude: f32,
    latitude: f32
}

#[derive(Serialize, Default, Clone)]
pub(super) struct Presence {
    pub logged_as_name: String,
    pub currently_present: bool,
    pub announced_dates: Vec<NaiveDate>,
}

#[derive(Deserialize)]
pub(super) struct PresenceAnnouncement {
    date: NaiveDate,
    site_ids: Vec<String>,
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

    update_userinfo(user_id, logged_as_name)?;

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
    "SELECT u.user_id, u.logged_as_name, to_char(a.present_on, 'YYYY-MM-DD')
    FROM user_announcements AS a JOIN user_info AS u ON a.user_id=u.user_id 
    WHERE a.site_id=$1
    
    UNION
    
    SELECT u.user_id, u.logged_as_name, NULL 
    FROM logged_into_site AS s JOIN user_info AS u ON s.user_id=u.user_id 
    WHERE s.site_id=$1 AND u.last_seen >= now() - INTERVAL '10 minutes'";

    let row_set = pg::query(&pg_address()?, &stmt, &[
        ParameterValue::Str(site_id),
    ])?;

    let presences: Vec<_> = row_set
    .rows.iter()
    .fold(HashMap::<String,Presence>::new(), |mut m,r|{
        let user_id = String::decode(&r[0]).unwrap();
        let present_on = if let DbValue::Str(s) = &r[2] {
            let date = NaiveDate::parse_from_str(&s, "%Y-%m-%d").unwrap();
            Some(date)
        } else {
            None
        };

        if !m.contains_key(&user_id) {
            m.insert(user_id.clone(), Presence {
                logged_as_name:String::decode(&r[1]).unwrap(), 
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

fn wrap_in_transaction<F,R>(pg_address: &str, f: F) -> Result<R>
where F: Fn() -> Result<R> + UnwindSafe
{
    pg::execute(pg_address, "BEGIN;", &[]).unwrap();
    match std::panic::catch_unwind(f) {
        Ok(f_result) => {
            match &f_result {
                &Ok(_) => pg::execute(pg_address, "COMMIT;", &[])?,
                &Err(_) => pg::execute(pg_address, "ROLLBACK;", &[])?,
            };

            f_result
        },
        Err(panic) => {
            pg::execute(pg_address, "ROLLBACK;", &[]).unwrap();

            std::panic::resume_unwind(panic)
        }
    }

}

fn update_userinfo(user_id: &str, logged_as_name: &str) -> Result<()> {
    let stmt = "INSERT INTO user_info (user_id, logged_as_name, last_seen) VALUES ($1, $2, now()) ON CONFLICT (user_id) 
    DO UPDATE SET logged_as_name=$2, last_seen=now()";
    pg::execute(&pg_address()?, stmt, &[
        ParameterValue::Str(user_id),
        ParameterValue::Str(logged_as_name)
        ]
    )?;
    Ok(())
}

pub(super) fn announce_presence_on_site(user_id: &str, logged_as_name: &str, announcements: &[PresenceAnnouncement]) -> Result<()> {

    wrap_in_transaction(&pg_address()?, move || {

        update_userinfo(user_id, logged_as_name)?;

        pg::execute(&pg_address()?, "DELETE FROM user_announcements WHERE user_id=$1", &[ParameterValue::Str(user_id)])?;

        let site_date_pairs: Vec<(&String, NaiveDate)> = announcements.iter()
            .flat_map(|a| a.site_ids.iter().map(|site|(site,a.date)))
            .collect();

        for (site_id, date) in site_date_pairs {
            let sql_date = date.format("%Y/%m/%d").to_string();
            
            let stmt = format!("INSERT INTO user_announcements (user_id, site_id, present_on) VALUES ($1, $2, '{}')", sql_date);
            pg::execute(&pg_address()?, &stmt, &[
                ParameterValue::Str(user_id),
                ParameterValue::Str(site_id)
            ])?;
        }

        Ok(())
    })
}