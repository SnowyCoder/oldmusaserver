use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::string::ToString;

use actix_files::NamedFile;
use actix_identity::Identity;
use actix_web::{error, Error, HttpResponse, web};
use futures::{future::{Either, err}, Future, Stream};

use crate::AppData;
use crate::models::{IdType, User};
use crate::security::PermissionCheckable;

use super::errors::{ServiceError, ServiceResult};

fn get_file_from_site(site_id: IdType) -> std::io::Result<PathBuf> {
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

pub fn image_download(ctx: web::Data<AppData>, identity: Identity, site_id: web::Path<IdType>) -> ServiceResult<NamedFile> {
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

pub fn image_upload(ctx: web::Data<AppData>, identity: Identity, site_id: web::Path<IdType>, payload: web::Payload) -> impl Future<Item = HttpResponse, Error = Error> {
    if let Err(x) = ensure_admin(&ctx, identity) {
        return Either::A(err(x.into()));
    };

    let file = match get_file_from_site(*site_id).and_then(fs::File::create) {
        Ok(file) => file,
        Err(e) => return Either::A(err(error::ErrorInternalServerError(e))),
    };

    Either::B(payload
        //.from_err()
        .fold((file, 0i64), move |(mut file, mut len), chunk| {
            web::block(move || {
                file.write_all(chunk.as_ref()).map_err(|e| {
                    eprintln!("file.write_all failed: {:?}", e);
                    error::PayloadError::Io(e)
                })?;
                len += chunk.len() as i64;
                Ok((file, len))
            })
                .map_err(|e: error::BlockingError<error::PayloadError>| {
                    match e {
                        error::BlockingError::Error(e) => e,
                        error::BlockingError::Canceled => error::PayloadError::Incomplete(None),
                    }
                })
        })
        .map(|(_, x)| HttpResponse::Ok().json(x))
        .map_err(|e| {
            eprintln!("save_file failed, {:?}", e);
            error::ErrorInternalServerError(e)
        }))
}

pub fn image_delete(ctx: web::Data<AppData>, identity: Identity, site_id: web::Path<IdType>) -> ServiceResult<()> {
    ensure_admin(&ctx, identity)?;
    get_file_from_site(*site_id)
        .map_err(|x| ServiceError::InternalServerError(x.to_string()))
        .and_then(|x| {
            if x.exists() {
                fs::remove_file(x).map_err(|x| ServiceError::InternalServerError(x.to_string()))
            } else { Err(ServiceError::NotFound("Image".to_string())) }
        })?;

    Ok(())
}
