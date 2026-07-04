use std::env;
use rocket::http::{Cookie, CookieJar, Status};
use rocket::response::{Redirect, status::Custom};
use rocket::serde::json::Json;
use rocket::State;
use rocket::request::{self, FromRequest, Outcome};
use rocket::Request;
use rocket::time::Duration;
use rocket_dyn_templates::{Template, context};
use webauthn_rs::prelude::*;
use url::Url;
use uuid::Uuid;

use crate::db::DbConn;
use crate::models::{PasskeyModel, ProfileModel};
use crate::actions;

pub struct AdminUser {
    pub username: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AdminUser {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        if let Some(cookie) = request.cookies().get_private("admin_logged_in") {
            Outcome::Success(AdminUser { username: cookie.value().to_string() })
        } else {
            Outcome::Error((Status::Unauthorized, ()))
        }
    }
}


pub fn init_webauthn() -> Webauthn {
    let rp_id = env::var("RP_ID").unwrap_or_else(|_| "localhost".to_string());
    let rp_origin_str = env::var("RP_ORIGIN").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let rp_origin = Url::parse(&rp_origin_str).expect("Invalid RP_ORIGIN");
    
    WebauthnBuilder::new(&rp_id, &rp_origin)
        .expect("Failed to create WebauthnBuilder")
        .build()
        .expect("Failed to build Webauthn")
}

#[get("/admin/login?<redirect>")]
pub fn admin_login(cookies: &CookieJar<'_>, redirect: Option<String>) -> Result<Template, Redirect> {
    let safe_redirect = redirect.filter(|r| is_valid_route(r));
    if cookies.get_private("admin_logged_in").is_some() {
        if let Some(r) = safe_redirect {
            return Err(Redirect::to(r));
        } else {
            return Err(Redirect::to(uri!(admin_dashboard)));
        }
    }
    Ok(Template::render("admin_login", context! {
        redirect: safe_redirect
    }))
}

#[derive(rocket::serde::Deserialize)]
pub struct AuthRequest {
    username: String,
}

#[post("/admin/login/start", format = "json", data = "<req>")]
pub fn login_start(
    mut conn: DbConn,
    webauthn: &State<Webauthn>,
    cookies: &CookieJar<'_>,
    req: Json<AuthRequest>,
) -> Result<Json<RequestChallengeResponse>, Custom<String>> {
    let name = req.username.trim();
    if name.is_empty() {
        return Err(Custom(Status::BadRequest, "Username cannot be empty".to_string()));
    }

    // Retrieve only authorized passkeys for this user
    let db_passkeys = actions::get_authorized_passkeys_by_username(&mut conn, name);
    if db_passkeys.is_empty() {
        return Err(Custom(
            Status::BadRequest,
            "No authorized passkeys found. If you just registered, you must authorize it via CLI first.".to_string(),
        ));
    }

    let mut credentials = Vec::new();
    for db_pk in db_passkeys {
        let pk: Passkey = serde_json::from_str(&db_pk.passkey)
            .map_err(|e| Custom(Status::InternalServerError, format!("Failed to parse passkey: {}", e)))?;
        credentials.push(pk);
    }

    let (rcr, auth_state) = webauthn.start_passkey_authentication(&credentials)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to start authentication: {}", e)))?;

    let state_json = serde_json::to_string(&auth_state)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to serialize auth state: {}", e)))?;

    let mut state_cookie = Cookie::new("auth_state", state_json);
    state_cookie.set_max_age(Some(Duration::minutes(2)));
    cookies.add_private(state_cookie);

    let mut user_cookie = Cookie::new("auth_username", name.to_string());
    user_cookie.set_max_age(Some(Duration::minutes(2)));
    cookies.add_private(user_cookie);

    Ok(Json(rcr))
}

#[post("/admin/login/finish", format = "json", data = "<credential>")]
pub fn login_finish(
    mut conn: DbConn,
    webauthn: &State<Webauthn>,
    cookies: &CookieJar<'_>,
    credential: Json<PublicKeyCredential>,
) -> Result<Status, Custom<String>> {
    let state_json = cookies.get_private("auth_state")
        .ok_or_else(|| Custom(Status::BadRequest, "Authentication session expired or not found".to_string()))?;
    let username = cookies.get_private("auth_username")
        .ok_or_else(|| Custom(Status::BadRequest, "Authentication username not found".to_string()))?;

    let auth_state: PasskeyAuthentication = serde_json::from_str(state_json.value())
        .map_err(|e| Custom(Status::BadRequest, format!("Invalid authentication state: {}", e)))?;

    let auth_res = webauthn.finish_passkey_authentication(&credential, &auth_state)
        .map_err(|e| Custom(Status::Unauthorized, format!("Passkey signature verification failed: {}", e)))?;

    // Find and update the matched passkey signature count and backup state
    let db_passkeys = actions::get_authorized_passkeys_by_username(&mut conn, username.value());
    let mut updated = false;

    for mut db_pk in db_passkeys {
        let mut pk: Passkey = serde_json::from_str(&db_pk.passkey)
            .map_err(|e| Custom(Status::InternalServerError, format!("Failed to parse passkey: {}", e)))?;

        if let Some(changed) = pk.update_credential(&auth_res) {
            if changed {
                db_pk.passkey = serde_json::to_string(&pk)
                    .map_err(|e| Custom(Status::InternalServerError, format!("Failed to serialize updated passkey: {}", e)))?;
                actions::update_passkey(&mut conn, db_pk)
                    .map_err(|e| Custom(Status::InternalServerError, format!("Failed to update passkey record: {}", e)))?;
            }
            updated = true;
            break;
        }
    }

    if !updated {
        return Err(Custom(Status::InternalServerError, "Could not find matching passkey record to update".to_string()));
    }

    // Clean up temporary session cookies
    cookies.remove_private(Cookie::new("auth_state", ""));
    cookies.remove_private(Cookie::new("auth_username", ""));

    // Log the user in
    cookies.add_private(Cookie::new("admin_logged_in", username.value().to_string()));

    Ok(Status::Ok)
}

#[get("/admin/register")]
pub fn admin_register(mut conn: DbConn, user: Option<AdminUser>) -> Result<Template, Redirect> {
    let has_auth = actions::has_any_authorized_passkey(&mut conn);
    if has_auth && user.is_none() {
        return Err(Redirect::to(uri!(admin_login(None::<String>))));
    }
    Ok(Template::render("admin_register", context! {
        has_existing_admin: has_auth
    }))
}

#[post("/admin/register/start", format = "json", data = "<req>")]
pub fn register_start(
    mut conn: DbConn,
    webauthn: &State<Webauthn>,
    cookies: &CookieJar<'_>,
    user: Option<AdminUser>,
    req: Json<AuthRequest>,
) -> Result<Json<CreationChallengeResponse>, Custom<String>> {
    let has_auth = actions::has_any_authorized_passkey(&mut conn);
    if has_auth && user.is_none() {
        return Err(Custom(Status::Unauthorized, "Registration is locked. You must be logged in as an admin to register new passkeys.".to_string()));
    }

    let name = req.username.trim();
    if name.is_empty() {
        return Err(Custom(Status::BadRequest, "Username cannot be empty".to_string()));
    }

    let user_id = Uuid::new_v4();

    let (ccr, reg_state) = webauthn.start_passkey_registration(user_id, name, name, None)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to start registration: {}", e)))?;

    let state_json = serde_json::to_string(&reg_state)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to serialize registration state: {}", e)))?;

    let mut state_cookie = Cookie::new("reg_state", state_json);
    state_cookie.set_max_age(Some(Duration::minutes(2)));
    cookies.add_private(state_cookie);

    let mut user_cookie = Cookie::new("reg_username", name.to_string());
    user_cookie.set_max_age(Some(Duration::minutes(2)));
    cookies.add_private(user_cookie);

    Ok(Json(ccr))
}

#[post("/admin/register/finish", format = "json", data = "<credential>")]
pub fn register_finish(
    mut conn: DbConn,
    webauthn: &State<Webauthn>,
    cookies: &CookieJar<'_>,
    user: Option<AdminUser>,
    credential: Json<RegisterPublicKeyCredential>,
) -> Result<String, Custom<String>> {
    let has_auth = actions::has_any_authorized_passkey(&mut conn);
    if has_auth && user.is_none() {
        return Err(Custom(Status::Unauthorized, "Registration is locked. You must be logged in as an admin to register new passkeys.".to_string()));
    }

    let state_json = cookies.get_private("reg_state")
        .ok_or_else(|| Custom(Status::BadRequest, "Registration session expired or not found".to_string()))?;
    let username = cookies.get_private("reg_username")
        .ok_or_else(|| Custom(Status::BadRequest, "Registration username not found".to_string()))?;

    let reg_state: PasskeyRegistration = serde_json::from_str(state_json.value())
        .map_err(|e| Custom(Status::BadRequest, format!("Invalid registration state: {}", e)))?;

    let passkey = webauthn.finish_passkey_registration(&credential, &reg_state)
        .map_err(|e| Custom(Status::BadRequest, format!("Passkey registration validation failed: {}", e)))?;

    let passkey_json = serde_json::to_string(&passkey)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to serialize passkey: {}", e)))?;

    let cred_id = credential.id.clone();

    let model = PasskeyModel {
        id: cred_id.clone(),
        username: username.value().to_string(),
        passkey: passkey_json,
        authorized: false, // Must be manually activated via CLI
    };

    actions::create_passkey(&mut conn, model)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to save passkey: {}", e)))?;

    // Clean up temporary cookies
    cookies.remove_private(Cookie::new("reg_state", ""));
    cookies.remove_private(Cookie::new("reg_username", ""));

    Ok(cred_id)
}

use rocket::form::Form;
use rocket::fs::TempFile;

#[derive(FromForm)]
pub struct PostForm {
    title: String,
    content: String,
}

#[derive(FromForm)]
pub struct NewPostForm<'r> {
    title: String,
    content: String,
    img_title: Option<String>,
    description: Option<String>,
    tag: Option<String>,
    file: Option<TempFile<'r>>,
}

#[derive(FromForm)]
pub struct ProfileForm {
    name: String,
    role: String,
    bio: String,
}

#[get("/admin/dashboard")]
pub fn admin_dashboard(mut conn: DbConn, user: AdminUser) -> Template {
    let posts = actions::get_all_posts_with_images(&mut conn);
    let profile = actions::get_profile(&mut conn);
    Template::render("admin_dashboard", context! {
        username: user.username,
        posts: posts,
        profile: profile
    })
}

#[post("/admin/profile", data = "<form>")]
pub fn update_profile_handler(
    mut conn: DbConn,
    _user: AdminUser,
    form: Form<ProfileForm>,
) -> Result<Redirect, Custom<String>> {
    let updated = ProfileModel {
        id: 1,
        name: form.name.clone(),
        role: form.role.clone(),
        bio: form.bio.clone(),
    };

    actions::update_profile(&mut conn, updated)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to update profile: {}", e)))?;

    Ok(Redirect::to(uri!(admin_dashboard)))
}

#[get("/admin/posts/new")]
pub fn new_post(user: AdminUser) -> Template {
    Template::render("admin_new_post", context! {
        username: user.username
    })
}

#[post("/admin/posts/new", data = "<form>")]
pub async fn create_post_handler(
    mut conn: DbConn,
    _user: AdminUser,
    mut form: Form<NewPostForm<'_>>,
) -> Result<Redirect, Custom<String>> {
    let new_post = crate::models::NewPost {
        title: form.title.clone(),
        content: form.content.clone(),
    };
    
    // 1. Insert post to get generated ID
    let post = actions::create_post(&mut conn, new_post)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to create post: {}", e)))?;
        
    // 2. Handle optional image upload
    if let Some(ref mut temp_file) = form.file {
        if temp_file.len() > 0 {
            let image_dir = std::env::var("IMAGE_DIR").unwrap_or_else(|_| "static".to_string());
            
            let ext = temp_file.content_type()
                .and_then(|ct| ct.extension())
                .map(|e| e.as_str())
                .unwrap_or("jpg");
                
            let filename = format!("{}.{}", Uuid::new_v4(), ext);
            let target_path = std::path::Path::new(&image_dir).join(&filename);
            
            // Persist the file with cross-device link fallback
            if let Err(e) = temp_file.persist_to(&target_path).await {
                if e.raw_os_error() == Some(18) {
                    if let Some(temp_path) = temp_file.path() {
                        std::fs::copy(temp_path, &target_path)
                            .map_err(|copy_err| Custom(Status::InternalServerError, format!("Failed to copy file across devices: {}", copy_err)))?;
                        let _ = std::fs::remove_file(temp_path);
                    } else {
                        return Err(Custom(Status::InternalServerError, format!("Failed to save uploaded file (temp path missing): {}", e)));
                    }
                } else {
                    return Err(Custom(Status::InternalServerError, format!("Failed to save uploaded file: {}", e)));
                }
            }
            
            let image_url = format!("/static/{}", filename);
            let (w, h) = match imagesize::size(&target_path) {
                Ok(dim) => (Some(dim.width as i32), Some(dim.height as i32)),
                Err(_) => (None, None),
            };
            
            let new_image = crate::models::NewImage {
                post_id: post.id,
                path: image_url,
                description: form.description.clone(),
                tag: form.tag.clone(),
                title: form.img_title.clone(),
                width: w,
                height: h,
            };
            
            actions::create_image(&mut conn, new_image)
                .map_err(|e| Custom(Status::InternalServerError, format!("Failed to save image record: {}", e)))?;
        }
    }
    
    Ok(Redirect::to(uri!(admin_dashboard)))
}

#[get("/digitally-distracted/<id>/edit")]
pub fn edit_post(mut conn: DbConn, _user: AdminUser, id: i32) -> Result<Template, Redirect> {
    match actions::get_post_with_images(&mut conn, id) {
        Some((post, images)) => {
            Ok(Template::render("admin_edit_post", context! {
                post: post,
                images: images
            }))
        }
        None => Err(Redirect::to(uri!(admin_dashboard)))
    }
}

#[post("/digitally-distracted/<id>/edit", data = "<form>")]
pub fn update_post_handler(
    mut conn: DbConn,
    _user: AdminUser,
    id: i32,
    form: Form<PostForm>,
) -> Result<Redirect, Custom<String>> {
    actions::update_post(&mut conn, id, form.title.clone(), form.content.clone())
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to update post: {}", e)))?;
    Ok(Redirect::to(uri!(admin_dashboard)))
}

#[post("/admin/posts/delete/<id>")]
pub fn delete_post_handler(
    mut conn: DbConn,
    _user: AdminUser,
    id: i32,
) -> Result<Redirect, Custom<String>> {
    if let Some((_, images)) = actions::get_post_with_images(&mut conn, id) {
        for img in images {
            delete_image_file(&img.path);
        }
    }
    
    actions::delete_post(&mut conn, id)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to delete post: {}", e)))?;
        
    Ok(Redirect::to(uri!(admin_dashboard)))
}

#[derive(FromForm)]
pub struct ImageUploadForm<'r> {
    img_title: Option<String>,
    description: Option<String>,
    tag: Option<String>,
    file: TempFile<'r>,
}

#[post("/digitally-distracted/<id>/edit/upload_image", data = "<form>")]
pub async fn upload_image_handler(
    mut conn: DbConn,
    _user: AdminUser,
    id: i32,
    mut form: Form<ImageUploadForm<'_>>,
) -> Result<Redirect, Custom<String>> {
    let image_dir = std::env::var("IMAGE_DIR").unwrap_or_else(|_| "static".to_string());
    
    let ext = form.file.content_type()
        .and_then(|ct| ct.extension())
        .map(|e| e.as_str())
        .unwrap_or("jpg");
        
    let filename = format!("{}.{}", Uuid::new_v4(), ext);
    let target_path = std::path::Path::new(&image_dir).join(&filename);
    
    if let Err(e) = form.file.persist_to(&target_path).await {
        if e.raw_os_error() == Some(18) { // EXDEV: Invalid cross-device link
            if let Some(temp_path) = form.file.path() {
                std::fs::copy(temp_path, &target_path)
                    .map_err(|copy_err| Custom(Status::InternalServerError, format!("Failed to copy file across devices: {}", copy_err)))?;
                let _ = std::fs::remove_file(temp_path);
            } else {
                return Err(Custom(Status::InternalServerError, format!("Failed to save uploaded file (temp path missing): {}", e)));
            }
        } else {
            return Err(Custom(Status::InternalServerError, format!("Failed to save uploaded file: {}", e)));
        }
    }
        
    let image_url = format!("/static/{}", filename);
    let (w, h) = match imagesize::size(&target_path) {
        Ok(dim) => (Some(dim.width as i32), Some(dim.height as i32)),
        Err(_) => (None, None),
    };
    
    let new_image = crate::models::NewImage {
        post_id: id,
        path: image_url,
        description: form.description.clone(),
        tag: form.tag.clone(),
        title: form.img_title.clone(),
        width: w,
        height: h,
    };
    
    actions::create_image(&mut conn, new_image)
        .map_err(|e| Custom(Status::InternalServerError, format!("Failed to save image record to DB: {}", e)))?;
        
    Ok(Redirect::to(uri!(edit_post(id))))
}

#[post("/digitally-distracted/<post_id>/edit/delete_image/<image_id>")]
pub fn delete_image_handler(
    mut conn: DbConn,
    _user: AdminUser,
    post_id: i32,
    image_id: i32,
) -> Result<Redirect, Custom<String>> {
    if let Some(img) = actions::get_image(&mut conn, image_id) {
        delete_image_file(&img.path);
        actions::delete_image(&mut conn, image_id)
            .map_err(|e| Custom(Status::InternalServerError, format!("Failed to delete image from DB: {}", e)))?;
    }
    Ok(Redirect::to(uri!(edit_post(post_id))))
}

fn delete_image_file(image_path: &str) {
    let image_dir = std::env::var("IMAGE_DIR").unwrap_or_else(|_| "static".to_string());
    use std::path::Path;
    if let Some(filename) = Path::new(image_path).file_name() {
        let file_path = Path::new(&image_dir).join(filename);
        if file_path.exists() {
            let _ = std::fs::remove_file(file_path);
        }
    }
}

#[get("/admin")]
pub fn admin_redirect() -> Redirect {
    Redirect::to(uri!(admin_dashboard))
}

#[post("/admin/logout")]
pub fn admin_logout(cookies: &CookieJar<'_>) -> Redirect {
    cookies.remove_private(Cookie::new("admin_logged_in", ""));
    Redirect::to(uri!(crate::index))
}

#[catch(401)]
pub fn unauthorized(req: &Request<'_>) -> Redirect {
    if req.method() == rocket::http::Method::Get {
        let redirect_uri = req.uri().to_string();
        if is_valid_route(&redirect_uri) && !redirect_uri.starts_with("/admin/login") {
            return Redirect::to(format!("/admin/login?redirect={}", redirect_uri));
        }
    }
    Redirect::to(uri!(admin_login(None::<String>)))
}

fn is_valid_route(path: &str) -> bool {
    let path_only = path.split('?').next().unwrap_or(path);
    if !path_only.starts_with('/') || path_only.starts_with("//") {
        return false;
    }
    
    let segments: Vec<&str> = path_only.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [] => true,
        ["digitally-distracted"] | ["admin"] => true,
        ["digitally-distracted", "partial"] => true,
        ["admin", "dashboard"] => true,
        ["admin", "login"] => true,
        ["admin", "register"] => true,
        ["digitally-distracted", id] => id.chars().all(|c| c.is_ascii_digit()),
        ["digitally-distracted", id, "edit"] => id.chars().all(|c| c.is_ascii_digit()),
        _ => false
    }
}
