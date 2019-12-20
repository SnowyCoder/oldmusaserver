use argonautica::{Hasher, Verifier};
use chrono::{prelude::*, Utc};
use diesel::{prelude::*, result::DatabaseErrorKind, result::Error as DBError};
use serde::{Deserialize, Serialize};

use crate::AppData;
use crate::models::{IdType, PermissionType, User, UserAccess};
use crate::schema::user_account;
use crate::web::errors::{ServiceError, ServiceResult};

pub fn hash_password(secret_key: &str, password: &str) -> Result<String, ServiceError> {
    Hasher::default()
        .with_password(password)
        .with_secret_key(secret_key)
        .hash()
        .map_err(|err| {
            dbg!(err.clone());
            ServiceError::InternalServerError(format!("Hashing error: {}", err))
        })
}

pub fn verify_hash(secret_key: &str, hash: &str, password: &str) -> bool {
    Verifier::default()
        .with_hash(hash)
        .with_password(password)
        .with_secret_key(secret_key)
        .verify()
        .map_err(|err| {
            // TODO: better error log
            dbg!(err)
        })
        .unwrap_or_else(|_| false)
}

#[derive(Insertable, AsChangeset)]
#[table_name="user_account"]
pub struct UserInputDb {
    pub username: Option<String>,
    pub password_hash: Option<String>,
    pub last_password_change: Option<chrono::NaiveDateTime>,
    pub permission: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdentityCookie {
    id: IdType,
    timestamp: NaiveDateTime,
}

#[derive(Clone)]
pub struct AuthCache {
    password_secret_key: String,
}

impl AuthCache {// TODO, implement a cache
    pub fn new(password_secret_key: String) -> Self {
        AuthCache {
            password_secret_key
        }
    }

    pub fn add_user(&self, ctx: &AppData, username: String, password: String, permission: PermissionType) -> ServiceResult<User> {
        use crate::schema::user_account::dsl;

        let now = Utc::now().naive_utc();
        let password_hash = hash_password(self.password_secret_key.as_str(), password.as_str())?;

        let value = UserInputDb {
            username: Some(username),
            password_hash: Some(password_hash),
            last_password_change: Some(now),
            permission: Some(permission.to_char().to_string()),
        };

        let conn = ctx.pool.get()?;

        Ok(diesel::insert_into(dsl::user_account)
            .values(value)
            .get_result(&conn)?)
    }

    fn find_user_by_username(&self, ctx: &AppData, username: String) -> ServiceResult<Option<User>> {
        use crate::schema::user_account::dsl;

        let conn = ctx.pool.get()?;
        Ok(dsl::user_account.filter(dsl::username.eq(username)).first::<User>(&conn).optional()?)
    }

    pub fn find_user_by_id(&self, ctx: &AppData, id: IdType) -> ServiceResult<Option<User>> {
        use crate::schema::user_account::dsl;

        let conn = ctx.pool.get()?;
        Ok(dsl::user_account.find(id).first::<User>(&conn).optional()?)
    }

    pub fn verify_user(&self, ctx: &AppData, username: String, password: String) -> ServiceResult<User> {
        let user = match self.find_user_by_username(ctx, username)? {
            None => return Err(ServiceError::NotFound("username".to_string())),
            Some(u) => u
        };

        if !verify_hash(self.password_secret_key.as_str(), user.password_hash.as_str(), password.as_str()) {
            Err(ServiceError::WrongPassword)
        } else {
            Ok(user)
        }
    }

    pub fn update_user(&self, ctx: &AppData, id: IdType, username: Option<String>, password: Option<String>, permission: Option<PermissionType>) -> ServiceResult<User> {
        use crate::schema::user_account::dsl;

        let (new_passw_hash, new_change_time) = match password {
            Some(x) => (
                Some(hash_password(self.password_secret_key.as_str(), x.as_str())?),
                Some(Utc::now().naive_utc())
            ),
            None => (None, None),
        };

        let data = UserInputDb {
            username,
            password_hash: new_passw_hash,
            last_password_change: new_change_time,
            permission: permission.map(|x| x.to_char().to_string())
        };

        let conn = ctx.pool.get()?;

        Ok(diesel::update(dsl::user_account.find(id))
            .set(&data)
            .get_result(&conn)?)
    }

    pub fn delete_user(&self, ctx: &AppData, id: IdType) -> ServiceResult<()> {
        use crate::schema::user_account::dsl;
        let conn = ctx.pool.get()?;

        let del_count = diesel::delete(dsl::user_account.find(id))
            .execute(&conn)?;

        if del_count != 1 {
            Err(ServiceError::NotFound("site".to_string()))
        } else {
            Ok(())
        }
    }

    pub fn give_access(&self, ctx: &AppData, user_id: IdType, site_id: IdType) -> ServiceResult<()> {
        use crate::schema::user_access::dsl;
        let conn = ctx.pool.get()?;

        let inserted = diesel::insert_into(dsl::user_access)
            .values(UserAccess { user_id, site_id })
            .on_conflict_do_nothing()
            .execute(&conn);


        match inserted {
            Err(DBError::DatabaseError(kind, info)) => match kind {
                DatabaseErrorKind::ForeignKeyViolation => Err(ServiceError::NotFound("user or site".to_string())),
                x => Err(DBError::DatabaseError(x, info).into()),
            },
            Err(x) => {
                Err(x.into())
            },
            Ok(insert_count) => {
                if insert_count == 0 {
                    Err(ServiceError::AlreadyPresent("Access".to_string()))
                } else {
                    Ok(())
                }
            },
        }
    }

    pub fn revoke_access(&self, ctx: &AppData, user_id: IdType, site_id: IdType) -> ServiceResult<()>{
        use crate::schema::user_access::dsl;
        let conn = ctx.pool.get()?;

        let deleted_count = diesel::delete(dsl::user_access)
            .filter(dsl::user_id.eq(user_id))
            .filter(dsl::site_id.eq(site_id))
            .execute(&conn)?;

        if deleted_count == 0 {
            Err(ServiceError::NotFound("user or site".to_string()))
        } else {
            Ok(())
        }
    }

    pub fn has_access(&self, ctx: &AppData, user_id: IdType, site_id: IdType) -> ServiceResult<bool> {
        use crate::schema::user_access::dsl;
        let conn = ctx.pool.get()?;

        let count: i64 = dsl::user_access
            .count()
            .filter(dsl::user_id.eq(user_id))
            .filter(dsl::site_id.eq(site_id))
            .get_result(&conn)?;

        Ok(count != 0)
    }

    pub fn ensure_access(&self, ctx: &AppData, user_id: IdType, site_id: IdType) -> ServiceResult<()> {
        if !self.has_access(ctx, user_id, site_id)? {
            Err(ServiceError::Unauthorized)
        } else {
            Ok(())
        }
    }

    pub fn save_identity(&self, user: &User) -> String {
        serde_json::to_string(&IdentityCookie {
            id: user.id,
            timestamp: user.last_password_change,
        }).unwrap()
    }

    pub fn parse_identity(&self, ctx: &AppData, identity: &str) -> ServiceResult<Option<User>> {
        let cookie: Option<IdentityCookie> = serde_json::from_str(identity).ok();
        let cookie = match cookie {
            Some(x) => x,
            None => return Ok(None),
        };

        let user = match self.find_user_by_id(ctx, cookie.id)? {
            None => return Ok(None),
            Some(u) => u,
        };
        if user.last_password_change > cookie.timestamp {
            Ok(None)
        } else {
            Ok(Some(user))
        }
    }
}

pub trait PermissionCheckable {
    fn ensure_admin(&self) -> ServiceResult<()>;

    fn ensure_site_visible(&self, ctx: &AppData, site_id: IdType) -> ServiceResult<()>;

    fn ensure_sensor_visible(&self, ctx: &AppData, sensor_id: IdType) -> ServiceResult<()>;

    fn ensure_channel_visible(&self, ctx: &AppData, channel_id: IdType) -> ServiceResult<()>;
}

impl PermissionCheckable for User {
    fn ensure_admin(&self) -> Result<(), ServiceError> {
        if PermissionType::from_char(self.permission.as_str()).unwrap_or(PermissionType::User) != PermissionType::Admin {
            Err(ServiceError::Unauthorized)
        } else {
            Ok(())
        }
    }

    fn ensure_site_visible(&self, ctx: &AppData, site_id: IdType) -> ServiceResult<()> {
        use crate::schema::user_access::dsl;
        if PermissionType::from_char(self.permission.as_str()) .unwrap_or(PermissionType::User) == PermissionType::Admin {
            return Ok(())
        }
        let conn = ctx.pool.get()?;

        let count: i64 = dsl::user_access.count()
            .filter(dsl::user_id.eq(self.id))
            .filter(dsl::site_id.eq(site_id))
            .get_result(&conn)?;

        if count == 0 {
            Err(ServiceError::NotFound("Site".to_string()))
        } else {
            Ok(())
        }
    }

    fn ensure_sensor_visible(&self, ctx: &AppData, sensor_id: IdType) -> ServiceResult<()> {
        use crate::schema::user_access::dsl;
        use crate::schema::sensor::dsl as sensor_dsl;
        if PermissionType::from_char(self.permission.as_str()) .unwrap_or(PermissionType::User) == PermissionType::Admin {
            return Ok(())
        }
        let conn = ctx.pool.get()?;

        let site_id = sensor_dsl::sensor
            .find(sensor_id)
            .select(sensor_dsl::site_id)
            .single_value();

        let count: i64 = dsl::user_access.count()
            .filter(dsl::user_id.eq(self.id))
            .filter(dsl::site_id.nullable().eq(site_id))
            .get_result(&conn)?;

        if count == 0 {
            Err(ServiceError::NotFound("Sensor".to_string()))
        } else {
            Ok(())
        }
    }

    fn ensure_channel_visible(&self, ctx: &AppData, channel_id: IdType) -> ServiceResult<()> {
        use crate::schema::user_access::dsl;
        use crate::schema::sensor::dsl as sensor_dsl;
        use crate::schema::channel::dsl as channel_dsl;
        if PermissionType::from_char(self.permission.as_str()) .unwrap_or(PermissionType::User) == PermissionType::Admin {
            return Ok(())
        }
        let conn = ctx.pool.get()?;

        let sensor_id = channel_dsl::channel
            .find(channel_id)
            .select(channel_dsl::sensor_id)
            .single_value();

        let site_id = sensor_dsl::sensor
            .filter(sensor_dsl::id.nullable().eq(sensor_id))
            .select(sensor_dsl::site_id)
            .single_value();

        let count: i64 = dsl::user_access.count()
            .filter(dsl::user_id.eq(self.id))
            .filter(dsl::site_id.nullable().eq(site_id))
            .get_result(&conn)?;

        if count == 0 {
            Err(ServiceError::NotFound("Channel".to_string()))
        } else {
            Ok(())
        }
    }
}


