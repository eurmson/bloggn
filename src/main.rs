#[macro_use]
extern crate rocket;

use rocket_dyn_templates::{Template, context};
use rocket::fs::FileServer;
mod actions;
mod db;
mod models;
mod schema; // This will be generated later
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
    actions::get_post_with_images(&mut conn, id).map(|(post, images)| {
        Template::render(
            "blog_post",
            context! {
                title: &post.title,
                content: &post.content,
                post: &post,
                images: &images
            },
        )
    })
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/", routes![index, blog, blog_partial, blog_post])
        .mount("/static", FileServer::from("static"))
        .attach(Template::fairing())
        .manage(db::init_pool()) // Add this line
}
