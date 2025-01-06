CREATE TABLE favorite_users (
    owner_user_id CHAR(36),
    favorite_user_id CHAR(36),
    PRIMARY KEY (owner_user_id,favorite_user_id)
);