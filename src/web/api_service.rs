use actix_web::web;

use super::graphql_service::{graphiql, graphql};
use super::site_map_service::{image_delete, image_download, image_upload};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .service(web::resource("/graphql").route(web::post().to(graphql)))
            .service(web::resource("/graphiql").route(web::get().to(graphiql)))
            .service(
                web::resource("/site_map/{site_id}")
                    .route(web::get().to(image_download))
                    .route(web::post().to(image_upload))
                    .route(web::delete().to(image_delete))
            )
    );
}