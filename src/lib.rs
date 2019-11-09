#[macro_use]
extern crate diesel;
extern crate dotenv;
#[macro_use]
extern crate juniper;

#[macro_use]
extern crate diesel_migrations;

use std::sync::Arc;

use actix_web::{HttpResponse, web};
use diesel::PgConnection;
use diesel::r2d2::ConnectionManager;

use crate::graphql_schema::{create_schema, Schema};
use crate::errors::ServiceResult;
use crate::models::PermissionType;
use diesel::prelude::*;

pub mod api_service;
pub mod errors;
pub mod graphql_schema;
pub mod graphql_service;
pub mod site_map_service;
pub mod schema;
pub mod schema_sensor;
pub mod models;
pub mod models_sensor;
pub mod security;

embed_migrations!();

#[derive(Clone)]
pub struct AppData {
    pub pool: models::Pool,
    pub sensor_pool: mysql::Pool,
    pub graphql_schema: Arc<Schema>,
    pub auth_cache: security::AuthCache,
}

impl AppData {
    pub fn new(password_secret_key: String, database_url: String, sensor_database_url: String) -> Self {
        let pool = {
            let manager = ConnectionManager::<PgConnection>::new(database_url);
            r2d2::Pool::builder()
                .build(manager)
                .expect("Failed to create pool")
        };
        let sensor_pool = mysql::Pool::new_manual(0, 10, sensor_database_url).unwrap();

        AppData {
            pool, sensor_pool,
            graphql_schema: Arc::new(create_schema()),
            auth_cache: security::AuthCache::new(password_secret_key)
        }
    }

    pub fn setup_migrations(&self) -> ServiceResult<()> {
        let conn = self.pool.get()?;
        embedded_migrations::run(&conn).unwrap();
        Ok(())
    }

    pub fn setup_root_password(&self, password: String, replace: bool) -> ServiceResult<()> {
        use crate::schema::user_account::dsl;
        use crate::models::User;

        let conn = self.pool.get()?;

        let user = dsl::user_account
            .filter(dsl::username.eq("root"))
            .first::<User>(&conn)
            .optional()?;

        std::mem::drop(conn);

        match user {
            None => {
                self.auth_cache.add_user(self, "root".to_string(), password, PermissionType::Admin)?;
            },
            Some(ref user) if replace => {
                self.auth_cache.update_user(self, user.id, None, Some(password), Some(PermissionType::Admin))?;
            },
            _ => {},
        }

        Ok(())
    }
}


fn get_test_sensor_data(ctx: web::Data<AppData>) -> Result<String, mysql::error::Error> {
    let result = ctx.sensor_pool.prep_exec("SELECT valore_min FROM t_rilevamento_dati LIMIT 100;", ());

    let datas: Vec<f64> = result.map(|qres| {
        qres.map(|row| {
            let min_value: f64 = mysql::from_row(row.unwrap());
            min_value
        }).collect()
    }).unwrap();

    Ok(format!("{:?}", datas))
}

pub fn test_sensor(
    ctx: web::Data<AppData>
) -> HttpResponse {
    let res: String =  match get_test_sensor_data(ctx) {
        Ok(s) => s,
        Err(x) => format!("Error: {:?}", x),
    };

    HttpResponse::Ok().content_type("text/html").body(res)
}

