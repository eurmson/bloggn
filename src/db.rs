use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use diesel::sqlite::SqliteConnection;
use rocket::outcome::Outcome;
use rocket::request::{self, FromRequest};
use rocket::{Request, State};
use std::ops::{Deref, DerefMut}; // Corrected tokio import

pub type SqlitePool = Pool<ConnectionManager<SqliteConnection>>;

pub fn connect() -> SqlitePool {
    let database_url = dotenvy::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let manager = ConnectionManager::<SqliteConnection>::new(database_url);
    Pool::builder()
        .build(manager)
        .expect("database connection pool could not be established.")
}

pub fn init_pool() -> SqlitePool {
    dotenvy::dotenv().ok();
    connect()
}

pub struct DbConn(PooledConnection<ConnectionManager<SqliteConnection>>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for DbConn {
    type Error = ();

    async fn from_request(request: &'r Request<'_>) -> request::Outcome<DbConn, Self::Error> {
        let pool = request
            .guard::<&State<SqlitePool>>()
            .await
            .succeeded()
            .expect("DB pool not attached to Rocket instance");
        let conn = pool.get().expect("Failed to get DB connection from pool");
        Outcome::Success(DbConn(conn))
    }
}

impl Deref for DbConn {
    type Target = SqliteConnection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for DbConn {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
