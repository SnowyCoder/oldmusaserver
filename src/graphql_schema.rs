extern crate dotenv;

use std::cell::RefCell;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use bigdecimal::{BigDecimal, ToPrimitive};
use diesel::{
    pg::PgConnection,
    prelude::*,
};
use diesel::r2d2::ConnectionManager;
use juniper::RootNode;
use r2d2::PooledConnection;

use crate::AppData;
use crate::errors::{ServiceError, ServiceResult};
use crate::models::{Channel, PermissionType, Sensor, Site, User, UserAccess, FcmUserContact};
use crate::schema::*;
use crate::security::PermissionCheckable;
use std::string::ToString;

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

fn load_user_sites(ctx: &Context, user_id: i32) -> ServiceResult<Vec<Site>> {
    use crate::schema::user_access::dsl as user_access;
    use crate::schema::site::dsl as site_dsl;

    let conn = ctx.get_connection()?;

    Ok(user_access::user_access.filter(user_access::user_id.eq(user_id))
        .inner_join(site_dsl::site)
        .select((site_dsl::id, site_dsl::name, site_dsl::id_cnr))
        .load::<Site>(&conn)?)
}

#[juniper::object(
    description = "An user account",
    Context = Context,
)]
impl User {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn username(&self) -> &str {
        self.username.as_str()
    }

    pub fn permission(&self) -> PermissionType {
        PermissionType::from_char(self.permission.as_str()).expect("Wrong permission found!")
    }


    // TODO: password management

    pub fn sites(&self, ctx: &Context) -> ServiceResult<Vec<Site>> {
        load_user_sites(ctx, self.id)
    }
}

#[juniper::object(
    description = "A site",
    Context = Context,
)]
impl Site {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|x| x.as_str())
    }

    pub fn id_cnr(&self) -> Option<&str> {
        self.id_cnr.as_ref().map(|x| x.as_str())
    }

    pub fn sensors(&self, ctx: &Context) -> ServiceResult<Vec<Sensor>> {
        use crate::schema::sensor::dsl::*;
        let connection = ctx.get_connection()?;
        // TODO: paging
        Ok(sensor.filter(site_id.eq(self.id))
            .load::<Sensor>(&connection)?)
    }
}

#[juniper::object(
    description = "A user access entry",
    Context = Context,
)]
impl UserAccess {
    pub fn user_id(&self) -> i32 {
        self.user_id
    }

    pub fn site_id(&self) -> i32 {
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
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn site_id(&self) -> i32 {
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

    pub fn status(&self) -> &str {
        self.status.as_str()
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
}

#[juniper::object(
    description = "A sensor channel",
    Context = Context,
)]
impl Channel {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn sensor_id(&self) -> i32 {
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

    pub fn sensor(&self, ctx: &Context) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl::*;
        let connection = ctx.get_connection()?;
        Ok(sensor.find(self.sensor_id).first::<Sensor>(&connection)?)
    }

    // TODO: data
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

    fn sites(ctx: &Context) -> ServiceResult<Vec<Site>> {
        let user = ctx.parse_user_required()?;

        // TODO: LIMIT
        let sites: Vec<Site> = match PermissionType::from_char(user.permission.as_str()).unwrap() {
            PermissionType::Admin => {
                use crate::schema::site::dsl::*;

                let conn = ctx.get_connection()?;
                site.load::<Site>(&conn)?
            },
            PermissionType::User => {
                load_user_sites(ctx, user.id)?
            }
        };

        Ok(sites)
    }

    fn site(ctx: &Context, id: i32) -> ServiceResult<Site> {
        use crate::schema::site::dsl;

        let user = ctx.parse_user_required()?;
        user.ensure_site_visible(&ctx.app, id)?;// TODO: single query?

        let conn = ctx.get_connection()?;

        let site: Site = dsl::site.find(id)
            .first::<Site>(&conn)
            .optional()
            .map_err(|x| ServiceError::from(x))?
            .ok_or(ServiceError::NotFound("Site".to_string()))?;
        Ok(site)
    }

    fn sensor(ctx: &Context, id: i32) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl;

        let user = ctx.parse_user_required()?;
        user.ensure_sensor_visible(&ctx.app, id)?;

        let conn = ctx.get_connection()?;

        let site: Sensor = dsl::sensor.find(id)
            .first::<Sensor>(&conn)
            .optional()
            .map_err(|x| ServiceError::from(x))?
            .ok_or(ServiceError::NotFound("Sensor".to_string()))?;
        Ok(site)
    }

    fn channel(ctx: &Context, id: i32) -> ServiceResult<Channel> {
        use crate::schema::channel::dsl;

        let user = ctx.parse_user_required()?;
        user.ensure_channel_visible(&ctx.app, id)?;

        let conn = ctx.get_connection()?;

        let site: Channel = dsl::channel.find(id)
            .first::<Channel>(&conn)
            .optional()
            .map_err(|x| ServiceError::from(x))?
            .ok_or(ServiceError::NotFound("Channel".to_string()))?;
        Ok(site)
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

#[derive(juniper::GraphQLInputObject, Insertable, AsChangeset)]
#[table_name="site"]
pub struct SiteInput {
    name: Option<String>,
    id_cnr: Option<String>,
}

#[derive(juniper::GraphQLInputObject, Insertable, AsChangeset)]
#[table_name="sensor"]
pub struct SensorInput {
    pub id_cnr: Option<String>,

    pub name: Option<String>,

    pub loc_x: Option<i32>,
    pub loc_y: Option<i32>,
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

    fn update_user(ctx: &Context, id: i32, data: UserUpdateInput) -> ServiceResult<User> {
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

    fn delete_user(ctx: &Context, id: i32) -> ServiceResult<bool> {
        let user = ctx.parse_user_required()?;
        user.ensure_admin()?;
        if user.id == id {
            return Err(ServiceError::Unauthorized)// TODO: different error
        }
        ctx.app.auth_cache.delete_user(&ctx.app, id)?;
        Ok(true)
    }

    fn give_user_access(ctx: &Context, user_id: i32, site_id: i32) -> ServiceResult<bool> {
        ctx.parse_user_required()?.ensure_admin()?;
        ctx.app.auth_cache.give_access(&ctx.app, user_id, site_id)?;
        Ok(true)
    }

    fn revoke_user_access(ctx: &Context, user_id: i32, site_id: i32) -> ServiceResult<bool> {
        ctx.parse_user_required()?.ensure_admin()?;
        ctx.app.auth_cache.revoke_access(&ctx.app, user_id, site_id)?;
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
    fn add_site(ctx: &Context, data: SiteInput) -> ServiceResult<Site> {
        use crate::schema::site::dsl::*;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        Ok(diesel::insert_into(site)
            .values(data)
            .get_result::<Site>(&conn)?)
    }

    fn update_site(ctx: &Context, id: i32, data: SiteInput) -> ServiceResult<Site> {
        use crate::schema::site::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        Ok(diesel::update(dsl::site.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    #[graphql(arguments(id(description = "Id of the site to delete")))]
    fn delete_site(ctx: &Context, id: i32) -> ServiceResult<bool> {
        use crate::schema::site::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let del_count = diesel::delete(dsl::site.find(id))
            .execute(&conn)?;

        if del_count != 1 {
            Err(ServiceError::NotFound("Site".to_string()))
        } else {
            Ok(true)
        }
    }

    fn add_sensor(ctx: &Context, site_id: i32, data: SensorInput) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        Ok(diesel::insert_into(dsl::sensor)
            .values((data, dsl::site_id.eq(site_id)))
            .get_result::<Sensor>(&conn)?)
    }

    fn update_sensor(ctx: &Context, id: i32, data: SensorInput) -> ServiceResult<Sensor> {
        use crate::schema::sensor::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        Ok(diesel::update(dsl::sensor.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    fn delete_sensor(ctx: &Context, id: i32) -> ServiceResult<bool> {
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

    fn add_channel(ctx: &Context, sensor_id: i32, data: ChannelInput) -> ServiceResult<Channel> {
        use crate::schema::channel::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let data: ChannelInputDb = data.into();

        Ok(diesel::insert_into(dsl::channel)
            .values((data, dsl::sensor_id.eq(sensor_id)))
            .get_result(&conn)?)
    }

    fn update_channel(ctx: &Context, id: i32, data: ChannelInput) -> ServiceResult<Channel> {
        use crate::schema::channel::dsl;

        ctx.parse_user_required()?.ensure_admin()?;
        let conn = ctx.get_connection()?;

        let data: ChannelInputDb = data.into();

        Ok(diesel::update(dsl::channel.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    fn delete_channel(ctx: &Context, id: i32) -> ServiceResult<bool> {
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
