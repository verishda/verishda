
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

CREATE TABLE user_info (
    user_id CHAR(36), 
    logged_as_name VARCHAR(127),
    last_seen timestamp,
    PRIMARY KEY(user_id)
);


CREATE TABLE user_announcements (
    user_id CHAR(36),
    site_id CHAR(36),
    present_on DATE,
    recurring BOOLEAN;
);
CREATE INDEX idx_user_announcements_user_id ON user_announcements (user_id);
CREATE INDEX idx_user_announcements_site_id ON user_announcements (site_id);