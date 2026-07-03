#[macro_use]
extern crate rocket;

use rocket_dyn_templates::{Template, context};
use rocket::fs::FileServer;
mod actions;
mod db;
mod models;
mod schema; // This will be generated later
mod auth;

use db::{DbConn, SqlitePool};

#[get("/")]
fn index() -> Template {
    Template::render(
        "index",
        context! {
            title: "My Blog",
            name: "Ethan Urmson",
            start_at_blog: false
        },
    )
}

#[get("/blog")]
fn blog(mut conn: DbConn) -> Template {
    let posts = actions::get_all_posts_with_images(&mut conn);
    Template::render(
        "index",
        context! {
            title: "My Blog",
            name: "User",
            posts: &posts,
            start_at_blog: true
        },
    )
}

#[get("/blog/partial")]
fn blog_partial(mut conn: DbConn) -> Template {
    let posts = actions::get_all_posts_with_images(&mut conn);
    Template::render(
        "posts_loop",
        context! {
            posts: &posts
        },
    )
}

#[get("/blog/<id>")]
fn blog_post(mut conn: DbConn, id: i32) -> Option<Template> {
    actions::get_single_post_with_images(&mut conn, id).map(|post| {
        Template::render(
            "blog_post",
            context! {
                title: &post.title,
                content: &post.content,
                post: &post,
                images: &post.images
            },
        )
    })
}

#[launch]
fn rocket() -> _ {
    dotenvy::dotenv().ok();

    // Command-line CLI interface for managing user passkey credentials
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        if args[1] == "--list-passkeys" {
            let pool = db::init_pool();
            let mut conn = pool.get().expect("Failed to get DB connection");
            
            use diesel::prelude::*;
            use crate::schema::passkeys::dsl::*;
            let list = passkeys.load::<crate::models::PasskeyModel>(&mut conn)
                .expect("Failed to load passkeys");
            
            println!("Registered Passkeys:");
            println!("------------------------------------------------------------");
            for pk in list {
                println!(
                    "ID: {} | User: {} | Authorized: {}", 
                    pk.id, pk.username, pk.authorized
                );
            }
            println!("------------------------------------------------------------");
            std::process::exit(0);
        } else if args[1] == "--authorize-id" && args.len() > 2 {
            let cred_id = &args[2];
            let pool = db::init_pool();
            let mut conn = pool.get().expect("Failed to get DB connection");
            
            use diesel::prelude::*;
            use crate::schema::passkeys::dsl::*;
            
            let count = diesel::update(passkeys.filter(id.eq(cred_id)))
                .set(authorized.eq(true))
                .execute(&mut conn)
                .expect("Failed to update passkeys");
                
            println!("Successfully authorized {} passkey(s) with ID '{}'.", count, cred_id);
            std::process::exit(0);
        } else if args[1] == "--backfill-images" {
            backfill_image_dimensions();
            std::process::exit(0);
        }
    }

    let image_dir = std::env::var("IMAGE_DIR").unwrap_or_else(|_| "static".to_string());
    let webauthn = auth::init_webauthn();

    rocket::build()
        .mount("/", routes![
            index, 
            blog, 
            blog_partial, 
            blog_post,
            auth::admin_login,
            auth::login_start,
            auth::login_finish,
            auth::admin_register,
            auth::register_start,
            auth::register_finish,
            auth::admin_dashboard,
            auth::admin_logout,
            auth::new_post,
            auth::create_post_handler,
            auth::edit_post,
            auth::update_post_handler,
            auth::delete_post_handler,
            auth::upload_image_handler,
            auth::delete_image_handler,
            auth::admin_redirect,
        ])
        .register("/", catchers![auth::unauthorized])
        .mount("/static", FileServer::from(image_dir))
        .attach(Template::fairing())
        .manage(db::init_pool())
        .manage(webauthn)
}

fn backfill_image_dimensions() {
    let pool = db::init_pool();
    let mut conn = pool.get().expect("Failed to get DB connection");
    
    use diesel::prelude::*;
    use crate::schema::images::dsl::*;
    let all_images = images.load::<crate::models::Image>(&mut conn).expect("Failed to load images");
    
    for img in all_images {
        let image_dir = std::env::var("IMAGE_DIR").unwrap_or_else(|_| "static".to_string());
        use std::path::Path;
        if let Some(filename) = Path::new(&img.path).file_name() {
            let file_path = if img.path.contains("/images/") {
                Path::new(&image_dir).join("images").join(filename)
            } else {
                Path::new(&image_dir).join(filename)
            };
            
            if file_path.exists() {
                if let Ok(dim) = imagesize::size(&file_path) {
                    let w = dim.width as i32;
                    let h = dim.height as i32;
                    println!("Backfilling image {}: {}x{}", img.path, w, h);
                    diesel::update(images.filter(id.eq(img.id)))
                        .set((
                            width.eq(Some(w)),
                            height.eq(Some(h)),
                        ))
                        .execute(&mut conn)
                        .expect("Failed to update image");
                } else {
                    println!("Failed to parse size for {}", file_path.display());
                }
            } else {
                println!("File not found: {}", file_path.display());
            }
        }
    }
}

