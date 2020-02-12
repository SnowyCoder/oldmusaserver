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

use crate::AppData;
use crate::models::{IdType, User};
use crate::security::PermissionCheckable;

use super::errors::{ServiceError, ServiceResult};

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
    mut payload: web::Payload
) -> Result<HttpResponse, Error> {
    if let Err(x) = ensure_admin(&ctx, identity) {
        return Err(x.into());
    };

    let mut file = match get_file_from_site(*site_id).and_then(fs::File::create) {
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
            Ok((file))
        }).await;
        file = res?;

        len += chunk_len;
    }
    // eprintln!("save_file failed, {:?}", e);
    //            error::ErrorInternalServerError(e)
    Ok(HttpResponse::Ok().json(len))
}

pub async fn image_delete(ctx: web::Data<AppData>, identity: Identity, site_id: web::Path<IdType>) -> ServiceResult<HttpResponse> {
    ensure_admin(&ctx, identity)?;
    get_file_from_site(*site_id)
        .map_err(|x| ServiceError::InternalServerError(x.to_string()))
        .and_then(|x| {
            if x.exists() {
                fs::remove_file(x).map_err(|x| ServiceError::InternalServerError(x.to_string()))
            } else { Err(ServiceError::NotFound("Image".to_string())) }
        })?;

    Ok(HttpResponse::new(StatusCode::NO_CONTENT))
}
