use bigdecimal::BigDecimal;
use derive_more::Display;
use diesel::{PgConnection, r2d2::ConnectionManager};

use super::schema::*;

// type alias to use in multiple places
pub type Pool = r2d2::Pool<ConnectionManager<PgConnection>>;

pub type IdType = i32;

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

#[derive(Clone, Debug, Queryable, Insertable)]
#[table_name = "user_account"]
pub struct User {
    pub id: IdType,
    pub username: String,
    pub password_hash: String,
    pub last_password_change: chrono::NaiveDateTime,
    pub permission: String,
}

#[derive(Debug, Queryable)]
pub struct Site {
    pub id: IdType,
    pub name: Option<String>,
    pub id_cnr: Option<String>,
    pub clock: chrono::NaiveDateTime,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
}
pub type SiteAllColumns = (
    site::dsl::id, site::dsl::name, site::dsl::id_cnr, site::dsl::clock, site::dsl::image_width,
    site::dsl::image_height
);
pub const SITE_ALL_COLUMNS: SiteAllColumns = (
    site::dsl::id, site::dsl::name, site::dsl::id_cnr, site::dsl::clock, site::dsl::image_width,
    site::dsl::image_height
);


#[derive(Debug, Queryable, Insertable)]
#[table_name="user_access"]
pub struct UserAccess {
    pub user_id: IdType,
    pub site_id: IdType,
}

#[derive(Debug, Queryable, Insertable)]
#[table_name="sensor"]
pub struct Sensor {
    pub id: IdType,
    pub site_id: IdType,
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub loc_x: Option<i32>,
    pub loc_y: Option<i32>,

    pub enabled: bool,
}
pub type SensorAllColumns = (
    sensor::dsl::id, sensor::dsl::site_id, sensor::dsl::id_cnr, sensor::dsl::name,
    sensor::dsl::loc_x, sensor::dsl::loc_y, sensor::dsl::enabled
);
pub const SENSOR_ALL_COLUMNS: SensorAllColumns = (
    sensor::dsl::id, sensor::dsl::site_id, sensor::dsl::id_cnr, sensor::dsl::name,
    sensor::dsl::loc_x, sensor::dsl::loc_y, sensor::dsl::enabled
);

#[derive(Debug, Queryable, Insertable)]
#[table_name="channel"]
pub struct Channel {
    pub id: IdType,
    pub sensor_id: IdType,
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub measure_unit: Option<String>,

    pub range_min: Option<BigDecimal>,
    pub range_max: Option<BigDecimal>,

    pub alarmed: bool,
}
pub type ChannelAllColumns = (
    channel::dsl::id, channel::dsl::sensor_id, channel::dsl::id_cnr, channel::dsl::name,
    channel::dsl::measure_unit, channel::dsl::range_min, channel::dsl::range_max,
    channel::dsl::alarmed
);
pub const CHANNEL_ALL_COLUMNS: ChannelAllColumns = (
    channel::dsl::id, channel::dsl::sensor_id, channel::dsl::id_cnr, channel::dsl::name,
    channel::dsl::measure_unit, channel::dsl::range_min, channel::dsl::range_max,
    channel::dsl::alarmed
);

#[derive(Debug, Queryable, Insertable)]
#[table_name="fcm_user_contact"]
pub struct FcmUserContact {
    pub registration_id: String,
    pub user_id: IdType,
}


