use crate::schema::*;
use chrono::NaiveDateTime as Timestamp;
use diesel::{Insertable, Queryable, Selectable, AsChangeset};
use serde::{Deserialize, Serialize}; // Explicitly added these

#[derive(Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = posts)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Post {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub published_at: Timestamp, // Changed from Timestamp to String
    pub published: bool,
}

#[derive(Insertable)]
#[diesel(table_name = posts)]
pub struct NewPost {
    pub title: String,
    pub content: String,
    pub published: bool,
}

#[derive(Insertable)]
#[diesel(table_name = images)]
pub struct NewImage {
    pub post_id: i32,
    pub path: String,
    pub description: Option<String>,
    pub tag: Option<String>,
    pub title: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

#[derive(Queryable, Selectable, Serialize)]
#[diesel(table_name = images)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Image {
    pub id: i32,
    pub post_id: i32,
    pub path: String,
    pub description: Option<String>,
    pub tag: Option<String>,
    pub title: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

// NewPost and NewImage will be added later when we re-enable creation

#[derive(Serialize)]
pub struct PostWithImages {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub published_at: Timestamp,
    pub published: bool,
    pub images: Vec<Image>,
    pub total_images: usize,
}

#[derive(Queryable, Selectable, Insertable, Serialize, Deserialize, Clone)]
#[diesel(table_name = passkeys)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct PasskeyModel {
    pub id: String,
    pub username: String,
    pub passkey: String, // JSON string of webauthn_rs::prelude::Passkey
    pub authorized: bool,
}

#[derive(Queryable, Selectable, Insertable, AsChangeset, Serialize, Deserialize, Clone)]
#[diesel(table_name = profile)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct ProfileModel {
    pub id: i32,
    pub name: String,
    pub role: String,
    pub bio: String,
}

