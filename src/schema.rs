// @generated automatically by Diesel CLI.

diesel::table! {
    images (id) {
        id -> Integer,
        post_id -> Integer,
        path -> Text,
        description -> Nullable<Text>,
        tag -> Nullable<Text>,
        title -> Nullable<Text>,
        width -> Nullable<Integer>,
        height -> Nullable<Integer>,
    }
}

diesel::table! {
    passkeys (id) {
        id -> Text,
        username -> Text,
        passkey -> Text,
        authorized -> Bool,
    }
}

diesel::table! {
    posts (id) {
        id -> Integer,
        title -> Text,
        content -> Text,
        published_at -> Timestamp,
        published -> Bool,
    }
}

diesel::table! {
    profile (id) {
        id -> Integer,
        name -> Text,
        role -> Text,
        bio -> Text,
    }
}

diesel::joinable!(images -> posts (post_id));

diesel::allow_tables_to_appear_in_same_query!(images, passkeys, posts, profile,);
