// @generated automatically by Diesel CLI.

diesel::table! {
    images (id) {
        id -> Integer,
        post_id -> Integer,
        path -> Text,
        description -> Nullable<Text>,
        tag -> Nullable<Text>,
    }
}

diesel::table! {
    posts (id) {
        id -> Integer,
        title -> Text,
        content -> Text,
        published_at -> Timestamp,
    }
}

diesel::joinable!(images -> posts (post_id));

diesel::allow_tables_to_appear_in_same_query!(images, posts,);
