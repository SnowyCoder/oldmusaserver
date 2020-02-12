use std::cell::RefCell;
use std::sync::Mutex;

use actix_identity::Identity;
use actix_web::{Error, HttpResponse, web, HttpRequest, http::Uri, http::PathAndQuery};
use juniper::http::{graphiql::graphiql_source, GraphQLRequest};

use crate::AppData;

use super::graphql_schema;

pub async fn graphql(
    ctx: web::Data<AppData>,
    identity: Identity,
    data: web::Json<GraphQLRequest>,
) -> Result<HttpResponse, Error> {
    let original_identity = identity.identity();

    let req_ctx = graphql_schema::Context {
        app: ctx.into_inner(),
        identity: Mutex::from(RefCell::from(original_identity.clone()))
    };

    // eprintln!("---------------------");
    // dbg!(data.clone());
    // eprintln!("---------------------");

    let (body, context) = web::block(move || {
        let res = data.execute(&req_ctx.app.graphql_schema, &req_ctx);
        Ok::<_, serde_json::error::Error>((serde_json::to_string(&res)?, req_ctx))
    }).await?;

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
}

pub fn graphiql(request: HttpRequest) -> HttpResponse {
    let mut orig = request.uri().clone().into_parts();
    orig.path_and_query = Some(PathAndQuery::from_static("/api/graphql"));
    let uri = Uri::from_parts(orig).expect("Cannot build URI");
    let html = graphiql_source(&uri.to_string());
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}
