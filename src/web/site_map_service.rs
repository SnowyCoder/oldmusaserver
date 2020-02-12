use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::string::ToString;

use actix_files::NamedFile;
use actix_identity::Identity;
use actix_web::{error, Error, HttpResponse, web};
use actix_web::error::BlockingError;
use actix_web::http::StatusCode;
use futures::StreamExt;
use serde::Deserialize;
use diesel::prelude::*;

use crate::AppData;
use crate::models::{IdType, User};
use crate::security::PermissionCheckable;

use super::errors::{ServiceError, ServiceResult};

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ImageSizeData {
    #[serde(rename = "width")]
    to_w: i32,
    #[serde(rename = "height")]
    to_h: i32,
}

pub fn get_file_from_site(site_id: IdType) -> std::io::Result<PathBuf> {
    let mut file_path = PathBuf::new();
    file_path.push("site_maps");
    if !file_path.exists() {
        fs::create_dir(&file_path)?;
    }
    file_path.push(format!("{}", site_id));
    Ok(file_path)
}

fn parse_user_required(ctx: &AppData, identity: Identity) -> ServiceResult<User> {
    Ok(identity.identity().as_ref()
        .and_then(|x| ctx.auth_cache.parse_identity(&ctx, x).transpose())
        .ok_or(ServiceError::LoginRequired)??)
}

fn ensure_admin(ctx: &AppData, identity: Identity) -> ServiceResult<()> {
    parse_user_required(ctx, identity)?.ensure_admin()
}

fn ensure_site_visible(ctx: &AppData, identity: Identity, site_id: IdType) -> ServiceResult<()> {
    parse_user_required(ctx, identity)?.ensure_site_visible(ctx, site_id)
}

pub async fn image_download(ctx: web::Data<AppData>, identity: Identity, site_id: web::Path<IdType>) -> ServiceResult<NamedFile> {
    ensure_site_visible(&ctx, identity, *site_id)?;
    let path = get_file_from_site(*site_id)
        .map_err(|x| ServiceError::InternalServerError(x.to_string()))
        .and_then(|path| {
            if path.exists() {
                NamedFile::open(path).map_err(|x| ServiceError::InternalServerError(x.to_string()))
            } else {
                Err(ServiceError::NotFound("Image".to_string()))
            }
        })?;

    Ok(path)
}

pub async fn image_upload(
    ctx: web::Data<AppData>,
    identity: Identity,
    site_id: web::Path<IdType>,
    mut payload: web::Payload,
    size_data: web::Query<ImageSizeData>
) -> Result<HttpResponse, Error> {
    use crate::schema::site::dsl as site_dsl;
    use crate::schema::sensor::dsl as sensor_dsl;

    let size: ImageSizeData = *size_data;

    if let Err(x) = ensure_admin(&ctx, identity) {
        return Err(x.into());
    };
    let site_id = *site_id;

    let mut file = match get_file_from_site(site_id).and_then(fs::File::create) {
        Ok(file) => file,
        Err(e) => return Err(error::ErrorInternalServerError(e)),
    };

    let mut len: i64 = 0;
    while let Some(chunk) = payload.next().await {
        let chunk = chunk?;
        let chunk_len = chunk.len() as i64;

        let res: Result<File, BlockingError<error::PayloadError>> = web::block(move || {
            file.write_all(chunk.as_ref()).map_err(|e| {
                eprintln!("file.write_all failed: {:?}", e);
                error::PayloadError::Io(e)
            })?;
            Ok(file)
        }).await;
        file = res?;

        len += chunk_len;
    }

    let conn =  ctx.pool.get()
        .map_err(ServiceError::from)?;

    let old_size_data: (Option<i32>, Option<i32>) = site_dsl::site.find(site_id)
        .select((site_dsl::image_width, site_dsl::image_height))
        .first::<(Option<i32>, Option<i32>)>(&conn)
        .map_err(ServiceError::from)?;

    if let (Some(old_w), Some(old_h)) = old_size_data {
        let mult_x = size.to_w / old_w;
        let mult_y = size.to_h / old_h;

        diesel::update(sensor_dsl::sensor.filter(sensor_dsl::site_id.eq(site_id)))
            .set((
                sensor_dsl::loc_x.eq(sensor_dsl::loc_x * mult_x),
                sensor_dsl::loc_y.eq(sensor_dsl::loc_y * mult_y)
            ))
            .execute(&conn)
            .map_err(ServiceError::from)?;
    }
    // Update image_width and image_height
    diesel::update(site_dsl::site.find(site_id))
        .set((
            site_dsl::image_width.eq(size.to_w),
            site_dsl::image_height.eq(size.to_h)
        ))
        .execute(&conn)
        .map_err(ServiceError::from)?;

    Ok(HttpResponse::Ok().json(len))
}

pub async fn image_delete(ctx: web::Data<AppData>, identity: Identity, site_id: web::Path<IdType>) -> ServiceResult<HttpResponse> {
    use crate::schema::site::dsl as site_dsl;
    use crate::schema::sensor::dsl as sensor_dsl;

    let site_id = *site_id;

    ensure_admin(&ctx, identity)?;
    get_file_from_site(site_id)
        .map_err(|x| ServiceError::InternalServerError(x.to_string()))
        .and_then(|x| {
            if x.exists() {
                fs::remove_file(x).map_err(|x| ServiceError::InternalServerError(x.to_string()))
            } else { Err(ServiceError::NotFound("Image".to_string())) }
        })?;


    let conn =  ctx.pool.get()
        .map_err(ServiceError::from)?;

    diesel::update(sensor_dsl::sensor.filter(sensor_dsl::site_id.eq(site_id)))
        .set((
            sensor_dsl::loc_x.eq(Option::<i32>::None),
            sensor_dsl::loc_y.eq(Option::<i32>::None)
        ))
        .execute(&conn)
        .map_err(ServiceError::from)?;

    diesel::update(site_dsl::site.find(site_id))
        .set((
            site_dsl::image_width.eq(Option::<i32>::None),
            site_dsl::image_height.eq(Option::<i32>::None)
        ))
        .execute(&conn)
        .map_err(ServiceError::from)?;

    Ok(HttpResponse::new(StatusCode::NO_CONTENT))
}
