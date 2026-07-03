use crate::models::{Image, Post, PostWithImages, NewPost, NewImage};
use crate::schema::{images, posts};
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

pub fn get_all_posts(conn: &mut SqliteConnection) -> Vec<Post> {
    posts::table
        .load::<Post>(conn)
        .expect("Error loading posts")
}

pub fn get_all_posts_with_images(conn: &mut SqliteConnection) -> Vec<PostWithImages> {
    let all_posts = posts::table
        .load::<Post>(conn)
        .expect("Error loading posts");

    let mut result = Vec::new();
    for post in all_posts {
        let associated_images = images::table
            .filter(images::post_id.eq(post.id))
            .load::<Image>(conn)
            .expect("Error loading images");
        
        result.push(process_post_images(post, associated_images));
    }
    result
}

pub fn get_single_post_with_images(
    conn: &mut SqliteConnection,
    post_id: i32,
) -> Option<PostWithImages> {
    let post = posts::table
        .filter(posts::id.eq(post_id))
        .first::<Post>(conn)
        .ok()?;

    let associated_images = images::table
        .filter(images::post_id.eq(post.id))
        .load::<Image>(conn)
        .ok()?;

    Some(process_post_images(post, associated_images))
}

fn process_post_images(post: Post, associated_images: Vec<Image>) -> PostWithImages {
    let total_count = associated_images.len();
    let mut final_content = post.content.clone();
    let mut bottom_images = Vec::new();

    for image in associated_images {
        let mut replaced = false;
        if let Some(ref tag) = image.tag {
            if !tag.is_empty() && final_content.contains(tag) {
                let desc_html = match &image.description {
                    Some(desc) if !desc.is_empty() => format!(r#"<p class="text-xs font-mono text-slate-500 text-center mt-2">— {}</p>"#, desc),
                    _ => String::new()
                };
                
                let display_name = match &image.title {
                    Some(t) if !t.trim().is_empty() => t.trim().to_string(),
                    _ => {
                        use std::path::Path;
                        Path::new(&image.path)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("image.jpg")
                            .to_string()
                    }
                };
                
                let size_attrs = match (image.width, image.height) {
                    (Some(w), Some(h)) => format!(r#" width="{}" height="{}" style="aspect-ratio: {} / {}; height: auto;""#, w, h, w, h),
                    _ => String::new()
                };

                let img_html = format!(
                    r#"<div class="mt-6 mb-1 select-none text-left"><div class="flex items-center gap-1 text-xs font-mono text-slate-500 mb-1.5"><span>$</span>display {}</div><img src="{}" alt="{}" class="block w-full object-cover border-0 p-0 rounded-none" loading="lazy"{}>{}</div>"#,
                    display_name,
                    image.path,
                    image.description.as_deref().unwrap_or("Blog Image"),
                    size_attrs,
                    desc_html
                );
                
                let double_newline_tag = format!("\n\n{}\n\n", tag);
                let single_newline_tag = format!("\n{}\n", tag);
                if final_content.contains(&double_newline_tag) {
                    final_content = final_content.replace(&double_newline_tag, &format!("\n{}", &img_html));
                } else if final_content.contains(&single_newline_tag) {
                    final_content = final_content.replace(&single_newline_tag, &format!("\n{}", &img_html));
                } else {
                    final_content = final_content.replace(tag, &img_html);
                }
                replaced = true;
            }
        }
        if !replaced {
            bottom_images.push(image);
        }
    }

    PostWithImages {
        id: post.id,
        title: post.title,
        content: final_content,
        published_at: post.published_at,
        images: bottom_images,
        total_images: total_count,
    }
}

pub fn get_post_with_images(
    conn: &mut SqliteConnection,
    post_id: i32,
) -> Option<(Post, Vec<Image>)> {
    let post = posts::table
        .filter(posts::id.eq(post_id))
        .first::<Post>(conn)
        .ok();

    if let Some(post) = post {
        let associated_images = images::table
            .filter(images::post_id.eq(post.id))
            .load::<Image>(conn)
            .expect("Error loading images for post");
        Some((post, associated_images))
    } else {
        None
    }
}

use crate::models::PasskeyModel;
use crate::schema::passkeys;

pub fn create_passkey(conn: &mut SqliteConnection, model: PasskeyModel) -> QueryResult<usize> {
    diesel::insert_into(passkeys::table)
        .values(&model)
        .execute(conn)
}

pub fn get_passkeys_by_username(conn: &mut SqliteConnection, name: &str) -> Vec<PasskeyModel> {
    passkeys::table
        .filter(passkeys::username.eq(name))
        .load::<PasskeyModel>(conn)
        .unwrap_or_default()
}

pub fn get_authorized_passkeys_by_username(conn: &mut SqliteConnection, name: &str) -> Vec<PasskeyModel> {
    passkeys::table
        .filter(passkeys::username.eq(name))
        .filter(passkeys::authorized.eq(true))
        .load::<PasskeyModel>(conn)
        .unwrap_or_default()
}

pub fn update_passkey(conn: &mut SqliteConnection, model: PasskeyModel) -> QueryResult<usize> {
    diesel::update(passkeys::table.filter(passkeys::id.eq(&model.id)))
        .set((
            passkeys::passkey.eq(&model.passkey),
            passkeys::authorized.eq(model.authorized),
        ))
        .execute(conn)
}

pub fn has_any_authorized_passkey(conn: &mut SqliteConnection) -> bool {
    use diesel::dsl::exists;
    use crate::schema::passkeys::dsl::*;
    
    diesel::select(exists(passkeys.filter(authorized.eq(true))))
        .get_result(conn)
        .unwrap_or(false)
}

pub fn create_post(conn: &mut SqliteConnection, new_post: NewPost) -> QueryResult<Post> {
    diesel::insert_into(posts::table)
        .values(&new_post)
        .execute(conn)?;
    
    posts::table
        .order(posts::id.desc())
        .first::<Post>(conn)
}

pub fn update_post(conn: &mut SqliteConnection, post_id: i32, new_title: String, new_content: String) -> QueryResult<usize> {
    diesel::update(posts::table.filter(posts::id.eq(post_id)))
        .set((
            posts::title.eq(new_title),
            posts::content.eq(new_content),
        ))
        .execute(conn)
}

pub fn delete_post(conn: &mut SqliteConnection, post_id: i32) -> QueryResult<usize> {
    // Also delete associated images from the DB (foreign key or manual)
    diesel::delete(images::table.filter(images::post_id.eq(post_id)))
        .execute(conn)?;
    diesel::delete(posts::table.filter(posts::id.eq(post_id)))
        .execute(conn)
}

pub fn create_image(conn: &mut SqliteConnection, new_image: NewImage) -> QueryResult<usize> {
    diesel::insert_into(images::table)
        .values(&new_image)
        .execute(conn)
}

pub fn delete_image(conn: &mut SqliteConnection, image_id: i32) -> QueryResult<usize> {
    diesel::delete(images::table.filter(images::id.eq(image_id)))
        .execute(conn)
}

pub fn get_image(conn: &mut SqliteConnection, image_id: i32) -> Option<Image> {
    images::table
        .filter(images::id.eq(image_id))
        .first::<Image>(conn)
        .ok()
}



