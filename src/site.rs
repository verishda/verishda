use anyhow::Result;
use serde::{Serialize};
use spin_sdk::pg::{ParameterValue, Decode, self};

#[derive(Serialize)]
pub(super) struct Site {
    name: String,
    longitude: f32,
    latitude: f32
}

pub(super) fn get_sites() -> Result<Vec<Site>> {
    let stmt = String::new() +
    "SELECT name, longitude, latitude FROM sites";

    let row_set = pg::query(&pg_address()?, &stmt, &[])?;
    let sites: Vec<_> = row_set
    .rows.iter()
    .map(|r|Site {
        name: String::decode(&r[0]).unwrap(), 
        longitude: f32::decode(&r[1]).unwrap(), 
        latitude: f32::decode(&r[2]).unwrap(),
    })
    .collect()
    ;

    Ok(sites)
}


fn pg_address() -> Result<String> {
    Ok(std::env::var("PG_ADDRESS")?)
}

pub(super) fn hello_site(user_id: &str, site_id: i32) -> Result<()>{
    let stmt = String::new() +
    "INSERT INTO logged_into_site VALUES ($1, $2, now()) ON CONFLICT (user_id) 
    DO UPDATE SET site_id=$2, last_seen=now()";

    let params = [
        ParameterValue::Str(user_id),
        ParameterValue::Int32(site_id),
    ];
    pg::execute(&pg_address()?, &stmt, &params).unwrap();

    Ok(())
}