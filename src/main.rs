use actix::prelude::*;
use actix_identity::{CookieIdentityPolicy, IdentityService};
use actix_web::{App, HttpServer, middleware, web};

use oldmusa_server::*;
use std::time::Duration;

fn expect_env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{} must be set", name))
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let database_url = expect_env_var("DATABASE_URL");
    let sensor_database_url = expect_env_var("SENSOR_DATABASE_URL");
    let cookie_secret_key = expect_env_var("COOKIE_SECRET_KEY");
    let password_secret_key = expect_env_var("PASSWORD_SECRET_KEY");

    let root_default_password = expect_env_var("ROOT_DEFAULT_PASSWORD");
    let root_password_override = std::env::var("ROOT_PASSWORD_OVERRIDE").map(|x| !x.is_empty()).unwrap_or(false);

    // create db connection pool
    let data = AppData::new(password_secret_key, database_url, sensor_database_url, contact::Contacter::new_from_env());
    let domain: String = std::env::var("DOMAIN").unwrap_or_else(|_| "localhost".to_string());

    data.setup_migrations().unwrap();
    data.setup_root_password(root_default_password, root_password_override).unwrap();

    let actor = alarm::AlarmActor {
        app_data: data.clone(),
        sleep_interval: Duration::from_secs(expect_env_var("MEASURE_CONTROL_SLEEP_TIME").parse().expect("Cannot parse MEASURE_CONTROL_SLEEP_TIME"))
    };
    actor.start();

    // Start http server
    HttpServer::new(move || {
        App::new()
            .data(data.clone())
            .wrap(IdentityService::new(
                // <- create identity middleware
                CookieIdentityPolicy::new(cookie_secret_key.as_bytes())    // <- create cookie identity policy
                    .name("auth-cookie")
                    .domain(domain.as_str())
                    .secure(false)))
            // enable logger
            .wrap(middleware::Logger::default())
            // limit the maximum amount of data that server will accept
            .data(web::JsonConfig::default().limit(4096))
            .configure(api_service::config)
            .service(web::resource("/stest").route(web::get().to(test_sensor)))
    })
        .bind("0.0.0.0:8080")?
        .run()
        .await
}
