extern crate dotenv;

use std::cell::RefCell;
use std::fs;
use std::ops::Deref;
use std::string::ToString;
use std::sync::{Arc, Mutex};

use bigdecimal::{BigDecimal, ToPrimitive};
use chrono::{NaiveDateTime, Utc};
use derive_more::Display;
use diesel::{
    pg::PgConnection,
    prelude::*,
};
use diesel::r2d2::ConnectionManager;
use juniper::RootNode;
use mysql::params;
use r2d2::PooledConnection;

use crate::AppData;
use crate::models::{Channel, CHANNEL_ALL_COLUMNS, FcmUserContact, IdType, PermissionType, Sensor,
                    SENSOR_ALL_COLUMNS, Site, SITE_ALL_COLUMNS, User, UserAccess};
use crate::schema::*;
use crate::security::PermissionCheckable;
use crate::web::db_helper::auto_create_sensor;
use crate::web::errors::ServiceError::InternalServerError;
use crate::web::site_map_service::get_file_from_site;

use super::db_helper::auto_create_site;
use super::errors::{ServiceError, ServiceResult};

pub struct Context {
    pub app: Arc<AppData>,
    pub identity: Mutex<RefCell<Option<String>>>,
}

impl Context {
    pub fn get_connection(&self) -> ServiceResult<PooledConnection<ConnectionManager<PgConnection>>> {
        Ok(self.app.pool.get()?)
    }

    pub fn parse_user(&self) -> ServiceResult<Option<User>> {
        let data_guard = self.identity.lock().unwrap();
        let data = data_guard.deref().borrow();
        let user = data.as_ref().and_then(|x| self.app.auth_cache.parse_identity(&self.app, x).transpose());
        Ok(user.transpose()?)
    }

    pub fn parse_user_required(&self) -> ServiceResult<User> {
        self.parse_user()?.ok_or(ServiceError::LoginRequired)
    }
}

impl juniper::Context for Context {}

#[derive(Debug, Display, juniper::GraphQLEnum, PartialEq)]
pub enum SensorStateType {
    Ok,
    Disabled,
    Alarm,
    Error,
}

#[derive(Debug, juniper::GraphQLObject, PartialEq)]
pub struct ReadingData {
    pub date: NaiveDateTime,
    pub value_min: f64,
    pub value_avg: Option<f64>,
    pub value_max: Option<f64>,
    pub deviation: Option<f64>,
    pub error: Option<String>,
}

fn load_user_sites(ctx: &Context, user_id: IdType) -> ServiceResult<Vec<Site>> {
    use crate::schema::user_access::dsl as user_access;
    use crate::schema::site::dsl as site_dsl;

    let conn = ctx.get_connection()?;

    Ok(user_access::user_access.filter(user_access::user_id.eq(user_id))
        .inner_join(site_dsl::site)
        .select(SITE_ALL_COLUMNS)
        .load::<Site>(&conn)?)
}

fn load_user_sites_filtered(ctx: &Context, user_id: IdType, ids: Vec<IdType>) -> ServiceResult<Vec<Site>> {
    use crate::schema::user_access::dsl as user_access;
    use crate::schema::site::dsl as site_dsl;

    let conn = ctx.get_connection()?;

    Ok(user_access::user_access.filter(user_access::user_id.eq(user_id))
        .inner_join(site_dsl::site)
        .filter(site_dsl::id.eq_any(ids))
        .select(SITE_ALL_COLUMNS)
        .load::<Site>(&conn)?)
}

#[juniper::object(
    description = "An user account",
    Context = Context,
)]
impl User {
    pub fn id(&self) -> IdType {
        self.id
    }

    pub fn username(&self) -> &str {
        self.username.as_str()
    }

    pub fn permission(&self) -> PermissionType {
        PermissionType::from_char(self.permission.as_str()).expect("Wrong permission found!")
    }

    pub fn sites(&self, ctx: &Context) -> ServiceResult<Vec<Site>> {
        load_user_sites(ctx, self.id)
    }
}

#[juniper::object(
    description = "A site",
    Context = Context,
)]
impl Site {
    pub fn id(&self) -> IdType {
        self.id
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|x| x.as_str())
    }

    pub fn id_cnr(&self) -> Option<&str> {
        self.id_cnr.as_ref().map(|x| x.as_str())
    }

    pub fn image_width(&self) -> Option<i32> {
        self.image_width
    }

    pub fn image_height(&self) -> Option<i32> {
        self.image_height
    }

    pub fn sensors(&self, ctx: &Context) -> ServiceResult<Vec<Sensor>> {
        use crate::schema::sensor::dsl::*;
        let connection = ctx.get_connection()?;
        // TODO: paging
        Ok(sensor.filter(site_id.eq(self.id))
            .load::<Sensor>(&connection)?)
    }

    /// Guesses the cnr sensor ids under this site based on recent readings,
    /// Admin privileges are required for this operation as it puts some stress on the database
    fn cnr_sensor_ids(&self, ctx: &Context) -> ServiceResult<Vec<String>> {
        ctx.parse_user_required()?.ensure_admin()?;
        let conn = &ctx.app.sensor_pool;

        let id_cnr = match self.id_cnr.as_ref() {
            None => return Ok(Vec::new()),
            Some(x) => x,
        };

        let res = conn.prep_exec("SELECT DISTINCT idsensore FROM (SELECT * FROM t_rilevamento_dati WHERE idsito = :site_id ORDER BY data DESC LIMIT 1000) AS tmp;", params!{
            "site_id" => id_cnr
        })?;
        let names: Vec<String> = res.map(|row| {
            mysql::from_row::<String>(row.unwrap())
        }).collect();

        Ok(names)
    }

    fn has_image(&self, ctx: &Context) -> ServiceResult<bool> {
        Ok(get_file_from_site(self.id)
            .map_err(|x| ServiceError::InternalServerError(x.to_string()))?
            .exists())
    }
}

#[juniper::object(
    description = "A user access entry",
    Context = Context,
)]
impl UserAccess {
    pub fn user_id(&self) -> IdType {
        self.user_id
    }

    pub fn site_id(&self) -> IdType {
        self.site_id
    }

    pub fn user(&self, ctx: &Context) -> ServiceResult<User> {
        use crate::schema::user_account::dsl::*;
        let connection = ctx.app.pool.get()?;
        Ok(user_account.find(self.user_id).first::<User>(&connection)?)
    }

    pub fn site(&self, ctx: &Context) -> ServiceResult<Site> {
        use crate::schema::site::dsl::*;
        let connection = ctx.get_connection()?;
        Ok(site.find(self.site_id).first::<Site>(&connection)?)
    }
}

#[juniper::object(
    description = "A sensor",
    Context = Context,
)]
impl Sensor {
    pub fn id(&self) -> IdType {
        self.id
    }

    pub fn site_id(&self) -> IdType {
        self.site_id
    }

    pub fn id_cnr(&self) -> Option<&str> {
        self.id_cnr.as_ref().map(|x| x.as_str())
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|x| x.as_str())
    }

    pub fn loc_x(&self) -> Option<i32> {
        self.loc_x
    }

    pub fn loc_y(&self) -> Option<i32> {
        self.loc_y
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn status(&self, ctx: &Context) -> ServiceResult<SensorStateType> {
        use crate::schema::channel::dsl::*;

        if !self.enabled {
            return Ok(SensorStateType::Disabled)
        }

        let connection = ctx.get_connection()?;

        let alarmed_count: i64 = channel.count()
            .filter(sensor_id.eq(self.id))
            .filter(alarmed.eq(true))
            .get_result(&connection)?;

        if alarmed_count > 0 {
            return Ok(SensorStateType::Alarm)
        }

        Ok(SensorStateType::Ok)
    }

    pub fn site(&self, ctx: &Context) -> ServiceResult<Site> {
        use crate::schema::site::dsl::*;
        let connection = ctx.get_connection()?;
        Ok(site.find(self.site_id).first::<Site>(&connection)?)
    }

    pub fn channels(&self, ctx: &Context) -> ServiceResult<Vec<Channel>> {
        use crate::schema::channel::dsl::*;
        let connection = ctx.get_connection()?;
        // TODO: paging
        Ok(channel.filter(sensor_id.eq(self.id))
            .load::<Channel>(&connection)?)
    }

    /// Guesses the cnr channel ids under this sensor based on recent readings,
    /// Admin privileges are required for this operation as it puts some stress on the database
    fn cnr_channel_ids(&self, ctx: &Context) -> ServiceResult<Vec<String>> {
        ctx.parse_user_required()?.ensure_admin()?;
        use crate::schema::site::dsl as site_dsl;

        let conn = &ctx.app.sensor_pool;

        let sensor_cnr_id = match self.id_cnr.as_ref() {
            None => return Ok(Vec::new()),
            Some(x) => x,
        };

        let connection = ctx.get_connection()?;
        let site_cnr_id = site_dsl::site.find(self.site_id)
            .select(site_dsl::id_cnr)
            .get_result::<Option<String>>(&connection)?;

        let site_cnr_id = match site_cnr_id {
            None => return Ok(Vec::new()),
            Some(x) => x,
        };

        let res = conn.prep_exec("SELECT DISTINCT canale FROM (SELECT * FROM t_rilevamento_dati WHERE idsito = :site_id AND idsensore = :sensor_id ORDER BY data DESC LIMIT 100) AS tmp;", params!{
            "site_id" => site_cnr_id,
            "sensor_id" => sensor_cnr_id,
        })?;
        let names: Vec<String> = res.map(|row| {
            mysql::from_row::<String>(row.unwrap())
        }).collect();

        Ok(names)
    }
}

impl Channel {
    fn query_cnr_ids(&self, ctx: &Context) -> ServiceResult<Option<(String, String, String)>> {
        use crate::schema::{
            channel::dsl as channel_dsl,
            sensor::dsl as sensor_dsl,
            site::dsl as site_dsl,
        };

        let conn = ctx.get_connection()?;

        let site_sensor = channel_dsl::channel.find(self.id)
            .inner_join(sensor_dsl::sensor.inner_join(site_dsl::site))
            .select((site_dsl::id_cnr, sensor_dsl::id_cnr))
            .get_result::<(Option<String>, Option<String>)>(&conn)?;

        let channel = self.id_cnr.clone();

        let res = if let ((Some(site_id), Some(sensor_id)), Some(channel_id)) = (site_sensor, channel) {
            Some((site_id, sensor_id, channel_id))
        } else { None };

        Ok(res)
    }
}

#[juniper::object(
    description = "A sensor channel",
    Context = Context,
)]
impl Channel {
    pub fn id(&self) -> IdType {
        self.id
    }

    pub fn sensor_id(&self) -> IdType {
        self.sensor_id
    }

    pub fn id_cnr(&self) -> Option<&str> {
        self.id_cnr.as_ref().map(|x| x.as_str())
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|x| x.as_str())
    }

    pub fn measure_unit(&self) -> Option<&str> {
        self.measure_unit.as_ref().map(|x| x.as_str())
    }


    pub fn range_min(&self) -> Option<f64> {
        self.range_min.as_ref().and_then(|x| x.to_f64())
    }

    pub fn range_max(&self) -> Option<f64> {
        self.range_max.as_ref().and_then(|x| x.to_f64())
    }

    pub fn alarmed(&self) -> bool {
        self.alarmed
    }

    pub fn sensor(&self, ctx: &Context) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl::*;
        let connection = ctx.get_connection()?;
        Ok(sensor.find(self.sensor_id).first::<Sensor>(&connection)?)
    }

    pub fn readings(&self, ctx: &Context, start: NaiveDateTime, end: NaiveDateTime) -> ServiceResult<Vec<ReadingData>> {
        let ids = self.query_cnr_ids(ctx)?;

        let ids = match ids {
            Some(x) => x,
            None => return Ok(Vec::new()),
        };

        let result = ctx.app.sensor_pool.prep_exec(
            "SELECT data, valore_min, valore_med, valore_max, scarto, errore FROM t_rilevamento_dati \
             WHERE data >= :start AND data <= :end AND idsito = :site_id AND idsensore = :sensor_id \
             AND canale = :channel_id;",
            params! {
            "start" => start,
            "end" => end,
            "site_id" => ids.0,
            "sensor_id" => ids.1,
            "channel_id" => ids.2,
        });

        let data: Vec<ReadingData> = result.map(|qres| {
            qres.map(|row| {
                let (date, value_min, value_avg, value_max, deviation, error) =
                    mysql::from_row::<(NaiveDateTime, f64, Option<f64>, Option<f64>, Option<f64>, Option<String>)>(row.unwrap());
                ReadingData {
                    date,
                    value_min,
                    value_avg,
                    value_max,
                    deviation,
                    error,
                }
            }).collect()
        }).map_err(|x| InternalServerError(x.to_string()))?;

        Ok(data)
    }
}


pub struct QueryRoot;

#[juniper::object(
    Context = Context
)]
impl QueryRoot {
    fn api_version() -> &str {
        "1.0"
    }

    fn user_me(ctx: &Context) -> ServiceResult<User> {
        ctx.parse_user_required()
    }

    fn users(ctx: &Context) -> ServiceResult<Vec<User>> {
        use crate::schema::user_account::dsl::*;
        ctx.parse_user_required()?.ensure_admin()?;

        let connection = ctx.get_connection()?;
        Ok(user_account.load::<User>(&connection)?)
    }

    fn sites(ctx: &Context, ids: Option<Vec<IdType>>) -> ServiceResult<Vec<Site>> {
        let user = ctx.parse_user_required()?;

        let len = ids.as_ref().map(|x| x.len());

        // TODO: LIMIT
        let sites: Vec<Site> = match PermissionType::from_char(user.permission.as_str()).unwrap() {
            PermissionType::Admin => {
                use crate::schema::site::dsl as site_dsl;

                let conn = ctx.get_connection()?;
                if let Some(filter_ids) = ids {
                    site_dsl::site.filter(site_dsl::id.eq_any(filter_ids)).load::<Site>(&conn)?
                } else {
                    site_dsl::site.load::<Site>(&conn)?
                }
            },
            PermissionType::User => {
                if let Some(filter_ids) = ids {
                    load_user_sites_filtered(ctx, user.id, filter_ids)?
                } else {
                    load_user_sites(ctx, user.id)?
                }
            }
        };

        if let Some(l) = len {
            if l != sites.len() {
                return Err(ServiceError::NotFound("Site".to_string()))
            }
        }

        Ok(sites)
    }

    fn sensors(ctx: &Context, ids: Vec<IdType>) -> ServiceResult<Vec<Sensor>> {
        use crate::schema::user_access::dsl as user_access;
        use crate::schema::site::dsl as site_dsl;
        use crate::schema::sensor::dsl as sensor_dsl;

        let user = ctx.parse_user_required()?;
        let conn = ctx.get_connection()?;

        let is_admin =  PermissionType::from_char(user.permission.as_str()).unwrap_or(PermissionType::User) == PermissionType::Admin;
        let ids_len = ids.len();

        let sensors = if is_admin {
            sensor_dsl::sensor
                .filter(sensor_dsl::id.eq_any(ids))
                .load::<Sensor>(&conn)?
        } else {
            user_access::user_access
                .filter(user_access::user_id.eq(user.id))
                .inner_join(site_dsl::site.inner_join(sensor_dsl::sensor))
                .filter(sensor_dsl::id.eq_any(ids))
                .select(SENSOR_ALL_COLUMNS)
                .load::<Sensor>(&conn)?
        };

        if sensors.len() != ids_len {
            return Err(ServiceError::NotFound("Sensor".to_string()))
        }
        Ok(sensors)
    }

    fn channels(ctx: &Context, ids: Vec<IdType>) -> ServiceResult<Vec<Channel>> {
        use crate::schema::user_access::dsl as user_access;
        use crate::schema::site::dsl as site_dsl;
        use crate::schema::sensor::dsl as sensor_dsl;
        use crate::schema::channel::dsl as channel_dsl;

        let user = ctx.parse_user_required()?;
        let conn = ctx.get_connection()?;

        let is_admin =  PermissionType::from_char(user.permission.as_str()).unwrap_or(PermissionType::User) == PermissionType::Admin;
        let ids_len = ids.len();

        let channels = if is_admin {
            channel_dsl::channel
                .filter(channel_dsl::id.eq_any(ids))
                .load::<Channel>(&conn)?
        } else {
            user_access::user_access
                .filter(user_access::user_id.eq(user.id))
                .inner_join(site_dsl::site.inner_join(sensor_dsl::sensor.inner_join(channel_dsl::channel)))
                .filter(channel_dsl::id.eq_any(ids))
                .select(CHANNEL_ALL_COLUMNS)
                .load::<Channel>(&conn)?
        };

        if channels.len() != ids_len {
            return Err(ServiceError::NotFound("Channel".to_string()))
        }
        Ok(channels)
    }

    fn user(ctx: &Context, id: IdType) -> ServiceResult<User> {
        let user = ctx.parse_user_required()?;

        if id == user.id {
            return Ok(user);
        }

        user.ensure_admin()?;// Only if the user didn't query himself

        match ctx.app.auth_cache.find_user_by_id(&ctx.app, id)? {
            Some(user) => Ok(user),
            None => Err(ServiceError::NotFound("User".to_string()))
        }
    }

    fn site(ctx: &Context, id: IdType) -> ServiceResult<Site> {
        use crate::schema::site::dsl;

        let user = ctx.parse_user_required()?;
        user.ensure_site_visible(&ctx.app, id)?;// TODO: single query?

        let conn = ctx.get_connection()?;

        let site: Site = dsl::site.find(id)
            .first::<Site>(&conn)
            .optional()
            .map_err(ServiceError::from)?
            .ok_or_else(|| ServiceError::NotFound("Site".to_string()))?;
        Ok(site)
    }

    fn sensor(ctx: &Context, id: IdType) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl;

        let user = ctx.parse_user_required()?;
        user.ensure_sensor_visible(&ctx.app, id)?;

        let conn = ctx.get_connection()?;

        let site: Sensor = dsl::sensor.find(id)
            .first::<Sensor>(&conn)
            .optional()
            .map_err(ServiceError::from)?
            .ok_or_else(|| ServiceError::NotFound("Sensor".to_string()))?;
        Ok(site)
    }

    fn channel(ctx: &Context, id: IdType) -> ServiceResult<Channel> {
        use crate::schema::channel::dsl;

        let user = ctx.parse_user_required()?;
        user.ensure_channel_visible(&ctx.app, id)?;

        let conn = ctx.get_connection()?;

        let site: Channel = dsl::channel.find(id)
            .first::<Channel>(&conn)
            .optional()
            .map_err(ServiceError::from)?
            .ok_or_else(|| ServiceError::NotFound("Channel".to_string()))?;
        Ok(site)
    }

    /// Guesses the cnr site ids using the readings on the database,
    /// Admin privileges are required for this operation as it puts some stress on the database
    fn cnr_site_ids(ctx: &Context) -> ServiceResult<Vec<String>> {
        ctx.parse_user_required()?.ensure_admin()?;
        let conn = &ctx.app.sensor_pool;

        let res = conn.prep_exec("SELECT DISTINCT idsito FROM t_rilevamento_dati;", ())?;
        let names: Vec<String> = res.map(|row| {
            mysql::from_row::<String>(row.unwrap())
        }).collect();

        Ok(names)
    }
}

pub struct MutationRoot;

#[derive(juniper::GraphQLInputObject)]
pub struct AuthInput {
    username: String,
    password: String,
}

#[derive(juniper::GraphQLInputObject)]
pub struct UserInput {
    username: String,
    password: String,
    permission: PermissionType,
}

#[derive(juniper::GraphQLInputObject)]
pub struct UserUpdateInput {
    username: Option<String>,
    password: Option<String>,
    permission: Option<PermissionType>,
}

#[derive(juniper::GraphQLInputObject)]
pub struct SiteCreateInput {
    name: Option<String>,
    id_cnr: Option<String>,
    auto_create: Option<bool>,
}

#[derive(juniper::GraphQLInputObject, Insertable, AsChangeset)]
#[table_name="site"]
pub struct SiteUpdateInput {
    name: Option<String>,
    id_cnr: Option<String>,
}

#[derive(juniper::GraphQLInputObject, Insertable, AsChangeset)]
#[table_name="sensor"]
pub struct SensorUpdateInput {
    pub id_cnr: Option<String>,

    pub name: Option<String>,
    pub enabled: Option<bool>,

    pub loc_x: Option<i32>,
    pub loc_y: Option<i32>,
}

#[derive(juniper::GraphQLInputObject)]
pub struct SensorCreateInput {
    pub id_cnr: Option<String>,

    pub name: Option<String>,
    pub enabled: Option<bool>,

    pub loc_x: Option<i32>,
    pub loc_y: Option<i32>,

    pub auto_create: Option<bool>,
}

#[derive(juniper::GraphQLInputObject)]
pub struct ChannelInput {
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub measure_unit: Option<String>,

    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
}

#[derive(Insertable, AsChangeset)]
#[table_name="channel"]
pub struct ChannelInputDb {
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub measure_unit: Option<String>,

    pub range_min: Option<BigDecimal>,
    pub range_max: Option<BigDecimal>,
}

impl From<ChannelInput> for ChannelInputDb {
    fn from(x: ChannelInput) -> ChannelInputDb {
        ChannelInputDb {
            id_cnr: x.id_cnr,
            name: x.name,
            measure_unit: x.measure_unit,
            range_min: x.range_min.map(|p| p.into()),
            range_max: x.range_max.map(|p| p.into()),
        }
    }
}


#[juniper::object(
    Context = Context
)]
impl MutationRoot {
    fn login(ctx: &Context, auth: AuthInput) -> ServiceResult<User> {
        let user = ctx.app.auth_cache.verify_user(&ctx.app, auth.username, auth.password)?;

        let identity = ctx.identity.lock().unwrap();
        let id_str = ctx.app.auth_cache.save_identity(&user);
        identity.replace(Some(id_str));
        Ok(user)
    }

    fn logout(ctx: &Context) -> bool {// Logout cannot fail
        let identity = ctx.identity.lock().unwrap();
        identity.replace(None);
        true
    }

    fn add_user(ctx: &Context, data: UserInput) -> ServiceResult<User> {
        ctx.parse_user_required()?.ensure_admin()?;
        ctx.app.auth_cache.add_user(&ctx.app, data.username, data.password, data.permission)
    }

    fn update_user(ctx: &Context, id: IdType, data: UserUpdateInput) -> ServiceResult<User> {
        let user = ctx.parse_user_required()?;

        if id != user.id || data.username.as_ref().is_some() || data.permission.as_ref().is_some() {
            user.ensure_admin()?
        }

        let own_password_changed = id == user.id && data.password.as_ref().is_some();

        let res = ctx.app.auth_cache.update_user(&ctx.app, id, data.username, data.password, data.permission)?;

        if own_password_changed {
            let identity = ctx.identity.lock().unwrap();
            let id_str = ctx.app.auth_cache.save_identity(&res);
            identity.replace(Some(id_str));
        }

        Ok(res)
    }

    fn delete_user(ctx: &Context, id: IdType) -> ServiceResult<bool> {
        let user = ctx.parse_user_required()?;
        user.ensure_admin()?;
        if user.id == id {
            return Err(ServiceError::Unauthorized)// TODO: different error
        }
        ctx.app.auth_cache.delete_user(&ctx.app, id)?;
        Ok(true)
    }

    fn give_user_access(ctx: &Context, user_id: IdType, site_ids: Vec<IdType>) -> ServiceResult<bool> {
        ctx.parse_user_required()?.ensure_admin()?;
        for site_id in site_ids {
            ctx.app.auth_cache.give_access(&ctx.app, user_id, site_id)?;
        }
        Ok(true)
    }

    fn revoke_user_access(ctx: &Context, user_id: IdType, site_ids: Vec<IdType>) -> ServiceResult<bool> {
        ctx.parse_user_required()?.ensure_admin()?;
        for site_id in site_ids {
            ctx.app.auth_cache.revoke_access(&ctx.app, user_id, site_id)?;
        }
        Ok(true)
    }

    fn add_fcm_contact(ctx: &Context, registration_id: String) -> ServiceResult<bool> {
        use crate::schema::fcm_user_contact::dsl;
        let user = ctx.parse_user_required()?;

        if registration_id.len() > 255 {
            return Err(ServiceError::BadRequest("registration_id too long".to_owned()))
        }

        let conn = ctx.get_connection()?;

        diesel::insert_into(dsl::fcm_user_contact)
            .values(FcmUserContact {
                registration_id,
                user_id: user.id,
            })
            .on_conflict_do_nothing()
            .execute(&conn)?;

        Ok(true)
    }

    fn delete_fcm_contact(ctx: &Context, registration_id: String) -> ServiceResult<bool> {
        use crate::schema::fcm_user_contact::dsl;
        let user = ctx.parse_user_required()?;

        if registration_id.len() > 255 {
            return Ok(true)// Not even going to query the db, the string cannot be present
        }

        let conn = ctx.get_connection()?;

        diesel::delete(dsl::fcm_user_contact)
            .filter(dsl::registration_id.eq(registration_id))
            .filter(dsl::user_id.eq(user.id))
            .execute(&conn)?;

        Ok(true)
    }

    #[graphql(arguments(data(description = "Initial site data")))]
    fn add_site(ctx: &Context, data: SiteCreateInput) -> ServiceResult<Site> {
        use crate::schema::site::dsl as site_dsl;

        ctx.parse_user_required()?.ensure_admin()?;

        let auto_create = data.auto_create.unwrap_or(false);
        if auto_create && data.id_cnr.is_none() {
            return Err(ServiceError::BadRequest("Trying to auto-create site without an id_cnr".to_string()))
        }

        let conn = ctx.get_connection()?;

        let now = Utc::now().naive_utc();

        let db_data = SiteUpdateInput {
            name: data.name,
            id_cnr: data.id_cnr.clone(),
        };

        let site = diesel::insert_into(site_dsl::site)
            .values((db_data, site_dsl::clock.eq(now)))
            .get_result::<Site>(&conn)?;

        if auto_create {
            auto_create_site(site.id, data.id_cnr.as_deref().unwrap_or(""), &conn, &ctx.app.sensor_pool)?;
        }

        Ok(site)
    }

    fn update_site(ctx: &Context, id: IdType, data: SiteUpdateInput) -> ServiceResult<Site> {
        use crate::schema::site::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        Ok(diesel::update(dsl::site.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    #[graphql(arguments(id(description = "Id of the site to delete")))]
    fn delete_site(ctx: &Context, id: IdType) -> ServiceResult<bool> {
        use crate::schema::site::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let del_count = diesel::delete(dsl::site.find(id))
            .execute(&conn)?;

        if del_count != 1 {
            return Err(ServiceError::NotFound("Site".to_string()))
        }

        // Delete site image
        let image_path = match get_file_from_site(id) {
            Ok(x) => x,
            Err(e) => return Err(ServiceError::InternalServerError(e.to_string())),
        };
        if image_path.exists() {
            fs::remove_file(image_path)
                .map_err(|x| ServiceError::InternalServerError(x.to_string()))?;
        }

        Ok(true)
    }

    fn add_sensor(ctx: &Context, site_id: IdType, data: SensorCreateInput) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl;

        ctx.parse_user_required()?.ensure_admin()?;

        let auto_create = data.auto_create.unwrap_or(false);
        if auto_create && data.id_cnr.is_none() {
            return Err(ServiceError::BadRequest("Trying to auto-create sensor without an id_cnr".to_string()))
        }

        let conn = ctx.get_connection()?;

        let db_data = SensorUpdateInput {
            id_cnr: data.id_cnr,
            name: data.name,
            enabled: data.enabled,
            loc_x: data.loc_x,
            loc_y: data.loc_y,
        };

        let res = diesel::insert_into(dsl::sensor)
            .values((db_data, dsl::site_id.eq(site_id)))
            .get_result::<Sensor>(&conn)?;

        if auto_create {
            use crate::schema::site::dsl as site_dsl;

            let site_cnr_id: Option<String> = site_dsl::site.find(site_id)
                .select(site_dsl::id_cnr)
                .get_result(&conn)?;

            auto_create_sensor(site_cnr_id.as_deref().unwrap_or(""), res.id, res.id_cnr.as_deref().unwrap_or(""), &conn, &ctx.app.sensor_pool)?;
        }

        Ok(res)
    }

    fn update_sensor(ctx: &Context, id: IdType, data: SensorUpdateInput) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        Ok(diesel::update(dsl::sensor.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    fn delete_sensor(ctx: &Context, id: IdType) -> ServiceResult<bool> {
        use crate::schema::sensor::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let del_count = diesel::delete(dsl::sensor.find(id))
            .execute(&conn)?;

        if del_count != 1 {
            Err(ServiceError::NotFound("Sensor".to_string()))
        } else {
            Ok(true)
        }
    }

    fn add_channel(ctx: &Context, sensor_id: IdType, data: ChannelInput) -> ServiceResult<Channel> {
        use crate::schema::channel::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let data: ChannelInputDb = data.into();

        Ok(diesel::insert_into(dsl::channel)
            .values((data, dsl::sensor_id.eq(sensor_id)))
            .get_result(&conn)?)
    }

    fn update_channel(ctx: &Context, id: IdType, data: ChannelInput) -> ServiceResult<Channel> {
        use crate::schema::channel::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let data: ChannelInputDb = data.into();

        Ok(diesel::update(dsl::channel.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    fn delete_channel(ctx: &Context, id: IdType) -> ServiceResult<bool> {
        use crate::schema::channel::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let del_count = diesel::delete(dsl::channel.find(id))
            .execute(&conn)?;

        if del_count != 1 {
            Err(ServiceError::NotFound("Channel".to_string()))
        } else {
            Ok(true)
        }
    }
}

pub type Schema = RootNode<'static, QueryRoot, MutationRoot>;

pub fn create_schema() -> Schema {
    Schema::new(QueryRoot {}, MutationRoot {})
}
