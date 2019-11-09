use std::cell::RefCell;
use std::sync::Mutex;

use actix_identity::Identity;
use actix_web::{Error, HttpResponse, web};
use futures::future::Future;
use juniper::http::{graphiql::graphiql_source, GraphQLRequest};

use crate::{AppData, graphql_schema};

pub fn graphql(
    ctx: web::Data<AppData>,
    identity: Identity,
    data: web::Json<GraphQLRequest>,
) -> impl Future<Item = HttpResponse, Error = Error> {
    let original_identity = identity.identity();

    let req_ctx = graphql_schema::Context {
        app: ctx.into_inner(),
        identity: Mutex::from(RefCell::from(original_identity.clone()))
    };

    // eprintln!("---------------------");
    // dbg!(data.clone());
    // eprintln!("---------------------");

    web::block(move || {
        let res = data.execute(&req_ctx.app.graphql_schema, &req_ctx);
        Ok::<_, serde_json::error::Error>((serde_json::to_string(&res)?, req_ctx))
    })
        .map_err(Error::from)
        .and_then(move |data| {
            let (body, context) = data;

            let new_identity = context.identity.into_inner().unwrap().into_inner();
            if new_identity != original_identity {
                match new_identity {
                    None => identity.forget(),
                    Some(x) => identity.remember(x),
                }
            }

            Ok(HttpResponse::Ok()
                .content_type("application/json")
                .body(body))
        })
}

pub fn graphiql() -> HttpResponse {
    let html = graphiql_source("http://localhost:8080/api/graphql");
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}
