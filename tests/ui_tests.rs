use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cookie::{Cookie as HttpCookie, CookieJar, Key};
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;
use thirtyfour::prelude::*;

struct TestContext {
    server_process: Child,
    driver_process: Child,
    test_db_path: String,
    log_path: String,
    driver: Option<WebDriver>,
    success: bool,
}

impl TestContext {
    fn driver(&self) -> &WebDriver {
        self.driver.as_ref().expect("WebDriver session has already been terminated")
    }

    async fn quit_driver(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(d) = self.driver.take() {
            d.quit().await?;
        }
        Ok(())
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        // Kill background processes when the test context drops
        let _ = self.server_process.kill();
        let _ = self.driver_process.kill();

        // Always leak the driver if it was not taken (indicating the test didn't close cleanly).
        // This avoids client-side hang/timeout loops trying to contact the killed driver.
        if let Some(d) = self.driver.take() {
            std::mem::forget(d);
        }

        if self.success && !thread::panicking() {
            // Clean up the temporary test files on success
            if Path::new(&self.test_db_path).exists() {
                let _ = fs::remove_file(&self.test_db_path);
            }
            if Path::new(&self.log_path).exists() {
                let _ = fs::remove_file(&self.log_path);
            }
        } else {
            // Keep DB and log file on failure, and print their paths
            println!(
                "\n[TEST-DEBUG] Test failed! Server logs saved to: {}\n[TEST-DEBUG] Test database saved to: {}",
                self.log_path, self.test_db_path
            );
        }
    }
}

// Expands user home directories (e.g. ~/Downloads)
fn expand_home(path: String) -> String {
    if path.starts_with("~/") {
        if let Ok(home) = env::var("HOME") {
            path.replacen("~", &home, 1)
        } else {
            path
        }
    } else {
        path
    }
}

// Spawns ChromeDriver silently on a given port if path is configured
fn spawn_chromedriver(port: u16) -> Option<(Child, String, Capabilities)> {
    dotenvy::dotenv().ok();
    let chrome_path = env::var("CHROMEDRIVER_PATH").ok()?;
    let full_path = expand_home(chrome_path);
    
    let child = Command::new(&full_path)
        .arg(format!("--port={}", port))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start ChromeDriver");
    
    let mut caps = DesiredCapabilities::chrome();
    let _ = caps.add_arg("--headless");
    let _ = caps.add_arg("--no-sandbox");
    let _ = caps.add_arg("--disable-dev-shm-usage");

    Some((child, format!("http://localhost:{}", port), caps.into()))
}

// Spawns GeckoDriver silently on a given port if path is configured
fn spawn_geckodriver(port: u16) -> Option<(Child, String, Capabilities)> {
    dotenvy::dotenv().ok();
    let gecko_path = env::var("GECKODRIVER_PATH").ok()?;
    let full_path = expand_home(gecko_path);
    
    let child = Command::new(&full_path)
        .arg(format!("--port={}", port))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start GeckoDriver");

    let mut caps = DesiredCapabilities::firefox();
    let _ = caps.add_arg("--headless");

    Some((child, format!("http://localhost:{}", port), caps.into()))
}

// Computes the cryptographically signed and encrypted private cookie value
fn get_signed_login_cookie_value() -> String {
    dotenvy::dotenv().ok();
    
    let secret_key_base64 = env::var("ROCKET_SECRET_KEY")
        .unwrap_or_else(|_| "i7DJj20DP8cqraea4OhLCWY+oJKa780VhW07Jihp9oI=".to_string());
        
    let key_bytes = BASE64.decode(secret_key_base64.trim())
        .expect("Failed to decode ROCKET_SECRET_KEY as Base64");
        
    let key = Key::derive_from(&key_bytes);
    
    let mut jar = CookieJar::new();
    let cookie = HttpCookie::new("admin_logged_in", "test_admin");
    jar.private_mut(&key).add(cookie);
    
    let encrypted_cookie = jar.get("admin_logged_in")
        .expect("Failed to encrypt test cookie");
        
    encrypted_cookie.value().to_string()
}

// Helper to initialize driver, server database, and log in
async fn setup_test_session(
    browser: &str,
    server_port: u16,
    webdriver_port: u16,
    db_name: &str,
) -> Option<TestContext> {
    // 1. Spawn browser driver
    let (driver_process, webdriver_url, caps) = match browser {
        "chrome" => spawn_chromedriver(webdriver_port)?,
        "firefox" => spawn_geckodriver(webdriver_port)?,
        _ => return None,
    };

    // Poll the Webdriver TCP port until it is open (wait up to 5 seconds)
    let mut driver_ready = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", webdriver_port)).is_ok() {
            driver_ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !driver_ready {
        println!("[TEST-DEBUG] Webdriver did not start on port {}", webdriver_port);
        return None;
    }

    // 2. Locate active Cargo target directory (e.g. target/debug or target/release)
    let target_dir = Path::new(env!("CARGO_BIN_EXE_bloggn"))
        .parent()
        .expect("Failed to find Cargo build output directory");

    let db_path = target_dir.join(db_name).to_string_lossy().into_owned();
    let log_path = target_dir
        .join(format!("{}_server.log", db_name.replace(".db", "")))
        .to_string_lossy()
        .into_owned();

    // 3. Prepare test database file
    if Path::new(&db_path).exists() {
        let _ = fs::remove_file(&db_path);
    }

    // 4. Prepare log file
    let log_file = fs::File::create(&log_path)
        .expect("Failed to create Rocket server log file");

    // 5. Compile/Launch Rocket Server
    let server_bin = env!("CARGO_BIN_EXE_bloggn");
    let server_process = Command::new(server_bin)
        .env("ROCKET_PORT", server_port.to_string())
        .env("DATABASE_URL", &db_path)
        .env("IMAGE_DIR", "static")
        .stdout(log_file.try_clone().expect("Failed to clone log file for stdout"))
        .stderr(log_file)
        .spawn()
        .expect("Failed to start Rocket test server");

    let mut ctx = TestContext {
        server_process,
        driver_process,
        test_db_path: db_path,
        log_path,
        driver: None,
        success: false,
    };

    // Poll the Rocket server TCP port until it is open (wait up to 10 seconds)
    let mut server_ready = false;
    for _ in 0..100 {
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", server_port)).is_ok() {
            server_ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !server_ready {
        println!("[TEST-DEBUG] Rocket server did not start on port {}", server_port);
        return None;
    }

    // 6. Connect WebDriver and log in by injecting authenticated cookie
    let driver = match WebDriver::new(&webdriver_url, caps).await {
        Ok(d) => d,
        Err(e) => {
            println!("[TEST-DEBUG] WebDriver::new failed to connect to {}: {:?}", webdriver_url, e);
            return None;
        }
    };

    if let Err(e) = driver.goto(format!("http://localhost:{}", server_port)).await {
        println!("[TEST-DEBUG] driver.goto home page failed: {:?}", e);
        return None;
    }

    let cookie_value = get_signed_login_cookie_value();
    let mut auth_cookie = thirtyfour::cookie::Cookie::new("admin_logged_in", cookie_value);
    auth_cookie.set_path("/");
    if let Err(e) = driver.add_cookie(auth_cookie).await {
        println!("[TEST-DEBUG] driver.add_cookie failed: {:?}", e);
        return None;
    }

    ctx.driver = Some(driver);
    Some(ctx)
}

// Helper to create a test post #1
async fn create_test_post(driver: &WebDriver, server_port: u16) -> Result<(), Box<dyn std::error::Error>> {
    driver.goto(format!("http://localhost:{}/admin/posts/new", server_port)).await?;
    
    // Explicit 3-second wait for page load / form fields
    let title_input = driver.query(By::Name("title"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    title_input.send_keys("Automated Test Title").await?;

    let content_area = driver.query(By::Name("content")).first().await?;
    content_area.send_keys("This is automated post body content. ").await?;

    let submit_btn = driver.query(By::Css("button[type='submit']")).first().await?;
    submit_btn.click().await?;
    
    // Wait until redirected page loads
    let _ = driver.query(By::Id("create-post-btn"))
        .wait(Duration::from_secs(5), Duration::from_millis(100))
        .first()
        .await?;
    Ok(())
}

// =========================================================================
// CHROME TESTS
// =========================================================================

#[tokio::test]
#[cfg_attr(not(has_chromedriver), ignore)]
async fn test_chrome_save_changes() -> Result<(), Box<dyn std::error::Error>> {
    let server_port = 8881;
    let webdriver_port = 9511;
    let db_name = "ui_test_chrome_save.db";

    let mut ctx = setup_test_session("chrome", server_port, webdriver_port, db_name)
        .await
        .expect("CHROMEDRIVER_PATH guaranteed to be set by cfg attribute");

    create_test_post(ctx.driver(), server_port).await?;

    // Go to edit page
    ctx.driver().goto(format!("http://localhost:{}/digitally-distracted/1/edit", server_port)).await?;

    // Explicit 3-second wait for editor page load
    let asterisk = ctx.driver().query(By::Id("unsaved-asterisk"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    assert!(!asterisk.is_displayed().await?, "Asterisk should be hidden initially");

    let content_area = ctx.driver().query(By::Id("content")).first().await?;
    content_area.send_keys("Edits here!").await?;
    assert!(asterisk.is_displayed().await?, "Asterisk should show up after modifying content");

    let save_btn = ctx.driver().query(By::XPath("//button[contains(text(), '$ save_changes')]")).first().await?;
    save_btn.click().await?;
    
    // Poll until the unsaved asterisk is hidden (CSS display: none)
    let mut check_count = 0;
    while check_count < 30 {
        let ast = ctx.driver().query(By::Id("unsaved-asterisk")).first().await?;
        if !ast.is_displayed().await? {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        check_count += 1;
    }

    let asterisk_after = ctx.driver().query(By::Id("unsaved-asterisk")).first().await?;
    assert!(!asterisk_after.is_displayed().await?, "Asterisk should hide again after saving");

    ctx.quit_driver().await?;
    ctx.success = true;
    Ok(())
}

#[tokio::test]
#[cfg_attr(not(has_chromedriver), ignore)]
async fn test_chrome_image_upload() -> Result<(), Box<dyn std::error::Error>> {
    let server_port = 8882;
    let webdriver_port = 9512;
    let db_name = "ui_test_chrome_upload.db";

    let mut ctx = setup_test_session("chrome", server_port, webdriver_port, db_name)
        .await
        .expect("CHROMEDRIVER_PATH guaranteed to be set by cfg attribute");

    create_test_post(ctx.driver(), server_port).await?;

    ctx.driver().goto(format!("http://localhost:{}/digitally-distracted/1/edit", server_port)).await?;

    // Wait explicitly for editor page
    let file_input = ctx.driver().query(By::Id("file"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    let project_dir = env::current_dir()?;
    let file_path = project_dir.join("favicon.svg");
    let abs_file_path = file_path.to_str().expect("invalid path string");
    file_input.send_keys(abs_file_path).await?;

    let img_title_input = ctx.driver().query(By::Id("img_title")).first().await?;
    img_title_input.send_keys("test_favicon").await?;

    let tag_input = ctx.driver().query(By::Id("tag")).first().await?;
    tag_input.send_keys("[img1]").await?;

    let upload_btn = ctx.driver().query(By::XPath("//button[contains(text(), '$ execute_upload')]")).first().await?;
    upload_btn.click().await?;

    // Implicit wait: driver.query().first() polls automatically until the tag button is rendered (wait max 10s)
    let insert_tag_btn = ctx.driver().query(By::ClassName("insert-tag-btn"))
        .wait(Duration::from_secs(10), Duration::from_millis(100))
        .first()
        .await?;
    assert_eq!(insert_tag_btn.text().await?, "[img1]");

    // Verify clicking tag inserts it
    let content_area = ctx.driver().query(By::Id("content")).first().await?;
    content_area.click().await?;
    insert_tag_btn.click().await?;

    let current_text = content_area.value().await?.unwrap_or_default();
    assert!(current_text.contains("[img1]"), "Content should have [img1]");

    ctx.quit_driver().await?;
    ctx.success = true;
    Ok(())
}

#[tokio::test]
#[cfg_attr(not(has_chromedriver), ignore)]
async fn test_chrome_image_delete() -> Result<(), Box<dyn std::error::Error>> {
    let server_port = 8883;
    let webdriver_port = 9513;
    let db_name = "ui_test_chrome_delete.db";

    let mut ctx = setup_test_session("chrome", server_port, webdriver_port, db_name)
        .await
        .expect("CHROMEDRIVER_PATH guaranteed to be set by cfg attribute");

    create_test_post(ctx.driver(), server_port).await?;

    ctx.driver().goto(format!("http://localhost:{}/digitally-distracted/1/edit", server_port)).await?;

    // Wait explicitly for editor page
    let file_input = ctx.driver().query(By::Id("file"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    let project_dir = env::current_dir()?;
    let file_path = project_dir.join("favicon.svg");
    let abs_file_path = file_path.to_str().expect("invalid path string");
    file_input.send_keys(abs_file_path).await?;

    let tag_input = ctx.driver().query(By::Id("tag")).first().await?;
    tag_input.send_keys("[img1]").await?;

    let upload_btn = ctx.driver().query(By::XPath("//button[contains(text(), '$ execute_upload')]")).first().await?;
    upload_btn.click().await?;

    // Wait implicitly for upload to complete (wait max 10s)
    let _insert_tag_btn = ctx.driver().query(By::ClassName("insert-tag-btn"))
        .wait(Duration::from_secs(10), Duration::from_millis(100))
        .first()
        .await?;

    // Mock window.confirm to automatically return true without showing the dialog
    ctx.driver().execute("window.confirm = function() { return true; };", vec![]).await?;

    let delete_btn = ctx.driver().query(By::Css(".delete-image-form button[type='submit']")).first().await?;
    delete_btn.click().await?;

    // Poll re-querying the DOM until the insert-tag-btn is completely removed
    let mut check_count = 0;
    while check_count < 30 {
        let rem_tag_btns = ctx.driver().query(By::ClassName("insert-tag-btn")).all_from_selector().await?;
        if rem_tag_btns.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        check_count += 1;
    }

    let rem_tag_btns = ctx.driver().query(By::ClassName("insert-tag-btn")).all_from_selector().await?;
    assert!(rem_tag_btns.is_empty(), "Media list should be empty");

    ctx.quit_driver().await?;
    ctx.success = true;
    Ok(())
}

// =========================================================================
// FIREFOX TESTS
// =========================================================================

#[tokio::test]
#[cfg_attr(not(has_geckodriver), ignore)]
async fn test_firefox_save_changes() -> Result<(), Box<dyn std::error::Error>> {
    let server_port = 8891;
    let webdriver_port = 4441;
    let db_name = "ui_test_firefox_save.db";

    let mut ctx = setup_test_session("firefox", server_port, webdriver_port, db_name)
        .await
        .expect("GECKODRIVER_PATH guaranteed to be set by cfg attribute");

    create_test_post(ctx.driver(), server_port).await?;

    ctx.driver().goto(format!("http://localhost:{}/digitally-distracted/1/edit", server_port)).await?;

    // Wait explicitly for editor page load
    let asterisk = ctx.driver().query(By::Id("unsaved-asterisk"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    assert!(!asterisk.is_displayed().await?, "Asterisk should be hidden initially");

    let content_area = ctx.driver().query(By::Id("content")).first().await?;
    content_area.send_keys("Edits here!").await?;
    assert!(asterisk.is_displayed().await?, "Asterisk should show up after modifying content");

    let save_btn = ctx.driver().query(By::XPath("//button[contains(text(), '$ save_changes')]")).first().await?;
    save_btn.click().await?;
    
    // Poll until the unsaved asterisk is hidden (CSS display: none)
    let mut check_count = 0;
    while check_count < 30 {
        let ast = ctx.driver().query(By::Id("unsaved-asterisk")).first().await?;
        if !ast.is_displayed().await? {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        check_count += 1;
    }

    let asterisk_after = ctx.driver().query(By::Id("unsaved-asterisk")).first().await?;
    assert!(!asterisk_after.is_displayed().await?, "Asterisk should hide again after saving");

    ctx.quit_driver().await?;
    ctx.success = true;
    Ok(())
}

#[tokio::test]
#[cfg_attr(not(has_geckodriver), ignore)]
async fn test_firefox_image_upload() -> Result<(), Box<dyn std::error::Error>> {
    let server_port = 8892;
    let webdriver_port = 4442;
    let db_name = "ui_test_firefox_upload.db";

    let mut ctx = setup_test_session("firefox", server_port, webdriver_port, db_name)
        .await
        .expect("GECKODRIVER_PATH guaranteed to be set by cfg attribute");

    create_test_post(ctx.driver(), server_port).await?;

    ctx.driver().goto(format!("http://localhost:{}/digitally-distracted/1/edit", server_port)).await?;

    // Wait explicitly for editor page
    let file_input = ctx.driver().query(By::Id("file"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    let project_dir = env::current_dir()?;
    let file_path = project_dir.join("favicon.svg");
    let abs_file_path = file_path.to_str().expect("invalid path string");
    file_input.send_keys(abs_file_path).await?;

    let img_title_input = ctx.driver().query(By::Id("img_title")).first().await?;
    img_title_input.send_keys("test_favicon").await?;

    let tag_input = ctx.driver().query(By::Id("tag")).first().await?;
    tag_input.send_keys("[img1]").await?;

    let upload_btn = ctx.driver().query(By::XPath("//button[contains(text(), '$ execute_upload')]")).first().await?;
    upload_btn.click().await?;

    let insert_tag_btn = ctx.driver().query(By::ClassName("insert-tag-btn"))
        .wait(Duration::from_secs(10), Duration::from_millis(100))
        .first()
        .await?;
    assert_eq!(insert_tag_btn.text().await?, "[img1]");

    let content_area = ctx.driver().query(By::Id("content")).first().await?;
    content_area.click().await?;
    insert_tag_btn.click().await?;

    let current_text = content_area.value().await?.unwrap_or_default();
    assert!(current_text.contains("[img1]"), "Content should have [img1]");

    ctx.quit_driver().await?;
    ctx.success = true;
    Ok(())
}

#[tokio::test]
#[cfg_attr(not(has_geckodriver), ignore)]
async fn test_firefox_image_delete() -> Result<(), Box<dyn std::error::Error>> {
    let server_port = 8893;
    let webdriver_port = 4443;
    let db_name = "ui_test_firefox_delete.db";

    let mut ctx = setup_test_session("firefox", server_port, webdriver_port, db_name)
        .await
        .expect("GECKODRIVER_PATH guaranteed to be set by cfg attribute");

    create_test_post(ctx.driver(), server_port).await?;

    ctx.driver().goto(format!("http://localhost:{}/digitally-distracted/1/edit", server_port)).await?;

    // Wait explicitly for editor page
    let file_input = ctx.driver().query(By::Id("file"))
        .wait(Duration::from_secs(3), Duration::from_millis(100))
        .first()
        .await?;
    let project_dir = env::current_dir()?;
    let file_path = project_dir.join("favicon.svg");
    let abs_file_path = file_path.to_str().expect("invalid path string");
    file_input.send_keys(abs_file_path).await?;

    let tag_input = ctx.driver().query(By::Id("tag")).first().await?;
    tag_input.send_keys("[img1]").await?;

    let upload_btn = ctx.driver().query(By::XPath("//button[contains(text(), '$ execute_upload')]")).first().await?;
    upload_btn.click().await?;

    let _insert_tag_btn = ctx.driver().query(By::ClassName("insert-tag-btn"))
        .wait(Duration::from_secs(10), Duration::from_millis(100))
        .first()
        .await?;

    // Mock window.confirm to automatically return true without showing the dialog
    ctx.driver().execute("window.confirm = function() { return true; };", vec![]).await?;

    let delete_btn = ctx.driver().query(By::Css(".delete-image-form button[type='submit']")).first().await?;
    delete_btn.click().await?;

    // Poll re-querying the DOM until the insert-tag-btn is completely removed
    let mut check_count = 0;
    while check_count < 30 {
        let rem_tag_btns = ctx.driver().query(By::ClassName("insert-tag-btn")).all_from_selector().await?;
        if rem_tag_btns.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        check_count += 1;
    }

    let rem_tag_btns = ctx.driver().query(By::ClassName("insert-tag-btn")).all_from_selector().await?;
    assert!(rem_tag_btns.is_empty(), "Media list should be empty");

    ctx.quit_driver().await?;
    ctx.success = true;
    Ok(())
}
