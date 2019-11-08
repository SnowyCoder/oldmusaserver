use bigdecimal::BigDecimal;
use derive_more::Display;
use diesel::{PgConnection, r2d2::ConnectionManager};

use super::schema::*;

// type alias to use in multiple places
pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;

#[derive(Debug, Display, juniper::GraphQLEnum, PartialEq)]
pub enum PermissionType {
    User,
    Admin
}

impl PermissionType {
    pub fn from_char(name: &str) -> Option<PermissionType> {
        match name {
            "u" => Some(PermissionType::User),
            "a" => Some(PermissionType::Admin),
            _ => None,
        }
    }

    pub fn to_char(&self) -> &str {
        match self {
            PermissionType::User => "u",
            PermissionType::Admin => "a",
        }
    }
}

#[derive(Debug, Queryable, Insertable)]
#[table_name = "user_account"]
pub struct User {
    pub id: i32,
    pub username: String,
    pub password_hash: String,
    pub last_password_change: chrono::NaiveDateTime,
    pub permission: String,
}

/*impl User {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn username(&self) -> &str {
        self.username.as_str()
    }

    pub fn permission(&self) -> char {
        self.permission
    }
}*/

#[derive(Debug, Queryable)]
pub struct Site {
    pub id: i32,
    pub name: Option<String>,
    pub id_cnr: Option<String>,
}


#[derive(Debug, Queryable, Insertable)]
#[table_name="user_access"]
pub struct UserAccess {
    pub user_id: i32,
    pub site_id: i32,
}

/*impl UserAccess {
    pub fn user_id(&self) -> i32 {
        return self.user_id
    }

    pub fn site_id(&self) -> i32 {
        self.site_id
    }
}*/

#[derive(Debug, Queryable, Insertable)]
#[table_name="sensor"]
pub struct Sensor {
    pub id: i32,
    pub site_id: i32,
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub loc_x: Option<i32>,
    pub loc_y: Option<i32>,

    pub enabled: bool,
    pub status: String,
}

#[derive(Debug, Queryable, Insertable)]
#[table_name="channel"]
pub struct Channel {
    pub id: i32,
    pub sensor_id: i32,
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub measure_unit: Option<String>,

    pub range_min: Option<BigDecimal>,
    pub range_max: Option<BigDecimal>,
}

#[derive(Debug, Queryable, Insertable)]
#[table_name="fcm_user_contact"]
pub struct FcmUserContact {
    pub registration_id: String,
    pub user_id: i32,
}


