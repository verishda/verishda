use std::{collections::HashMap, ops::Range};

use anyhow::{anyhow,Result};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Utc};
use sqlx::{Connection, Postgres, PgConnection, postgres::PgRow, Row};

use crate::verishda_dto::types::{Presence, PresenceAnnouncement, PresenceAnnouncementKind, Site};

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

fn range_to_sql_offset_limit(range: Range<i32>, reserve_first: bool) -> (i32, i32) {
    let offset;
    let limit;
    if reserve_first {
        offset = i32::max(0, i32::saturating_sub(range.start, 1));
        limit = range.end - range.start - 1;
    } else {
        offset = range.start;
        limit = range.end - range.start;
    }

    (offset, limit)
}

#[test]
fn test_range_to_offset_limit() {
    assert_eq!(
        range_to_sql_offset_limit(0..0, false),
        (0,0)
    );
    assert_eq!(
        range_to_sql_offset_limit(0..i32::MAX, false),
        (0,i32::MAX)
    );

    assert_eq!(
        range_to_sql_offset_limit(0..i32::MAX, true),
        (0,i32::MAX-1)
    );
    assert_eq!(
        range_to_sql_offset_limit(0..10, true),
        (0,9)
    );
    assert_eq!(
        range_to_sql_offset_limit(1..10, true),
        (0,8)
    );
    assert_eq!(
        range_to_sql_offset_limit(2..10, true),
        (1,7)
    );
    assert_eq!(
        range_to_sql_offset_limit(3..10, true),
        (2,6)
    );

}

fn pgrow_to_userid_presence(r: &PgRow, self_user_id: &str) -> (String, Presence) {
    let last_seen: Option<NaiveDateTime> = r.get(2);
    let presence_user_id: String = r.get::<Option<String>,_>(0).unwrap();
    let is_self = presence_user_id == self_user_id;
    let five_minutes_ago = Utc::now().naive_local().checked_sub_signed(TimeDelta::minutes(5)).unwrap();
    let is_favorite = r.get::<Option<bool>,_>(3).unwrap();
    let presence = Presence{
        user_id: presence_user_id.clone(),
        announcements: Vec::new(),
        currently_present: last_seen.filter(|d|five_minutes_ago < *d).is_some(),
        is_self,
        logged_as_name: r.get::<Option<String>,_>(1).unwrap(),
        is_favorite,
    };

    (presence_user_id, presence)
}

fn self_presence_from_name(user_id: &str, logged_as_name: &str) -> Presence {
    Presence{
        user_id: user_id.to_owned(),
        currently_present: false,
        is_favorite: false,
        logged_as_name: logged_as_name.to_string(),
        announcements: Vec::new(),
        is_self: true,
    }
}

pub async fn add_favorite(pg: &mut PgConnection, user_id: &str, favorite_user_id: &str) -> Result<()> {
    sqlx::query("
        INSERT INTO favorite_users (owner_user_id,favorite_user_id) SELECT u.user_id, $2 FROM user_info AS u WHERE u.user_id=$1;
        ")
        .bind(user_id)
        .bind(favorite_user_id)
        .execute(pg)
        .await?
        ;
    Ok(())
}

pub async fn remove_favorite(pg: &mut PgConnection, user_id: &str, favorite_user_id: &str) -> Result<()> {
    if user_id == favorite_user_id {
        return Err(anyhow!("cannot add yourself as favorite"));
    }
    sqlx::query("
        DELETE FROM favorite_users WHERE owner_user_id=$1 AND favorite_user_id=$2;
        ")
        .bind(user_id)
        .bind(favorite_user_id)
        .execute(pg)
        .await?
        ;
    Ok(())
}

pub async fn get_presence_on_site(pg: &mut PgConnection, user_id: &str, logged_as_name: &str, site_id: &str, range: Range<i32>, term: Option<&str>, favorites_only: bool) -> Result<Vec<Presence>> {

    let mut tr = pg.begin().await?;

    // build offset limit from range and handle empty case without query
    if range.is_empty() {
        return Ok(Vec::new())
    }

    log::debug!("fetching user infos..");

    let self_user_at_start = term.is_none();
    let term = term.map(&str::to_string).unwrap_or(String::new());

    // get user infos with presence info, but without announcements, 
    // if we need it (result window starting from index 0, self user at start of list)
    let self_user_infos;     
    if self_user_at_start && range.start == 0 {
        let row = sqlx::query(
            "
            SELECT u.user_id, u.logged_as_name, l.last_seen, FALSE
            FROM user_info AS u
            LEFT JOIN logged_into_site AS l ON l.user_id=u.user_id AND l.site_id=$1
            WHERE u.user_id = $2
            "
        )
        .bind(site_id)
        .bind(user_id)
        .fetch_optional(&mut *tr).await?;

        // map existing self user to Presence, or if not found
        // update userinfo and return synthetic presence
        self_user_infos = row
        .map(|row|pgrow_to_userid_presence(&row, user_id))
        .or_else(||{
            Some((user_id.to_owned(), self_presence_from_name(user_id, logged_as_name)))
        })
        
    } else {
        self_user_infos = None
    };

    let (offset, limit) = range_to_sql_offset_limit(range, self_user_at_start);

    let exclude_user_id = self_user_at_start;

    let user_infos = sqlx::query(
        "
        SELECT u.user_id, u.logged_as_name, l.last_seen, f.owner_user_id IS NOT NULL
        FROM user_info AS u
        LEFT JOIN logged_into_site AS l ON l.user_id=u.user_id AND l.site_id=$2
        LEFT JOIN favorite_users AS f ON f.owner_user_id=$5 AND u.user_id=f.favorite_user_id
        WHERE ($1='' OR lower(u.logged_as_name) LIKE concat('%',lower($1),'%')) 
        AND ($6 IS FALSE OR u.user_id <> $5)
        AND ($7 IS FALSE OR f.owner_user_id IS NOT NULL)
        ORDER BY logged_as_name
        OFFSET $3 LIMIT $4
        "
    )
    .bind(term)
    .bind(site_id)
    .bind(offset as i32)
    .bind(limit as i32)
    .bind(user_id)
    .bind(exclude_user_id)
    .bind(favorites_only)
    .fetch_all(&mut *tr).await?;

    let user_infos = user_infos
    .iter()
    .map(|r|pgrow_to_userid_presence(r, user_id))
    .collect::<Vec<(String,Presence)>>()
    ;

    log::debug!("{} user infos fetched", user_infos.len());

    // build presence objects in-order, in a tuple with the user_id (which is not part of Presence)
    let presences = self_user_infos.iter()
    .chain(user_infos.iter())
    .collect::<Vec<&(String,Presence)>>();

    // query announcements for all user_ids in presences and build a map, mapping 
    // user_ids to Vecs of Announcements
    let user_ids = (&presences).iter().map(|p|p.0.clone()).collect::<Vec<_>>();
    let mut user_announcements = sqlx::query("
        SELECT a.user_id, a.present_on, a.recurring
        FROM user_announcements AS a
        WHERE a.site_id=$1 AND a.user_id = ANY($2)
    ")
    .bind(site_id)
    .bind(&user_ids)
    .fetch_all(&mut *tr).await.expect("cannot fetch announcements")
    .iter()
    .fold(HashMap::<String,Vec<PresenceAnnouncement>>::new(), |mut m, r|{
        let user_id: String = r.get::<String,_>(0);
        let present_on: NaiveDate = r.get::<NaiveDate,_>(1);

        let recurring = r.get::<bool,_>(2);
        if !m.contains_key(&user_id) {
            m.insert(user_id.clone(), Vec::new());
        }
        let announcements = m.get_mut(&user_id).unwrap();

        announcements.push(PresenceAnnouncement { 
            date: present_on,
            kind: if recurring {
                PresenceAnnouncementKind::RecurringAnnouncement
            } else {
                PresenceAnnouncementKind::SingularAnnouncement
            }
        });

        m
    });
    log::debug!("{} user announcements fetched and pre-processed", user_announcements.len());

    // assemble presences and announcements, if any
    let presences = presences.iter()
    .map(|(user_id,p)|{
        let mut presence = p.clone();
        if let Some(a) = user_announcements.remove(user_id) {
            presence.announcements = a;
        }
        presence
    })
    .collect();
    
    return Ok(presences)
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
        let recurring = a.kind == PresenceAnnouncementKind::RecurringAnnouncement;

        let stmt = format!("INSERT INTO user_announcements (user_id, site_id, present_on, recurring) VALUES ($1, $2, '{}', $3)", sql_date);
        sqlx::query(&stmt)
        .bind(&user_id.to_string())
        .bind(&site_id.to_string())
        .bind(recurring)
        .execute(&mut *tr)
        .await?;
    }
    Ok(tr.commit().await?)
}