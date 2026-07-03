use crate::models::{Image, Post, PostWithImages};
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
                    
                    use std::path::Path;
                    let filename = Path::new(&image.path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("image.jpg");
                    
                    let img_html = format!(
                        r#"<div class="mt-6 mb-1 select-none text-left"><div class="flex items-center gap-1 text-xs font-mono text-slate-500 mb-1.5"><span>$</span>display {}</div><img src="{}" alt="{}" class="block w-full object-cover border-0 p-0 rounded-none" loading="lazy">{}</div>"#,
                        filename,
                        image.path,
                        image.description.as_deref().unwrap_or("Blog Image"),
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

        result.push(PostWithImages {
            id: post.id,
            title: post.title,
            content: final_content,
            published_at: post.published_at,
            images: bottom_images,
        });
    }
    result
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

