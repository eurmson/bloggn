use crate::schema::*;
use chrono::NaiveDateTime as Timestamp;
use diesel::{Insertable, Queryable, Selectable};
use serde::{Deserialize, Serialize}; // Explicitly added these

#[derive(Queryable, Selectable, Serialize, Deserialize)]
#[diesel(table_name = posts)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct Post {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub published_at: Timestamp, // Changed from Timestamp to String
}

#[derive(Insertable)]
#[diesel(table_name = posts)]
pub struct NewPost {
    pub title: String,
    pub content: String,
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
}

// NewPost and NewImage will be added later when we re-enable creation

#[derive(Serialize)]
pub struct PostWithImages {
    pub id: i32,
    pub title: String,
    pub content: String,
    pub published_at: Timestamp,
    pub images: Vec<Image>,
}
