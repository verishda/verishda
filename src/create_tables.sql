
CREATE TABLE sites (
    id CHAR(36), 
    name VARCHAR(63), 
    latitude REAL, 
    longitude REAL,
    PRIMARY KEY(id)
);

CREATE TABLE logged_into_site (
    user_id CHAR(36), 
    logged_as_name VARCHAR(127),
    site_id CHAR(36) REFERENCES sites(id), 
    last_seen timestamp,
    PRIMARY KEY(user_id)
);
