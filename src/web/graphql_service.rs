use actix_identity::Identity;
use actix_web::{Error, http::PathAndQuery, http::Uri, HttpRequest, HttpResponse, web};
use juniper::http::{graphiql::graphiql_source, GraphQLRequest};

use crate::AppData;

use super::graphql_schema;
use std::time::Instant;

pub async fn graphql(
    ctx: web::Data<AppData>,
    identity: Identity,
    data: web::Json<GraphQLRequest>,
) -> Result<HttpResponse, Error> {
    let original_identity = identity.identity();
    let user = original_identity.as_ref()
        .and_then(|x| ctx.auth_cache.parse_identity(&ctx, x).transpose())
        .transpose()?;

    let req_quota = if let (Some(bank), Some(user)) = (&ctx.quota_bank, &user) {
        bank.get_quota_balance(Instant::now(), user.id)
    } else {
        i64::max_value()
    };

    let req_ctx = graphql_schema::Context::new(ctx.into_inner(), original_identity.clone(), user, req_quota);

    let (body, context) = web::block(move || {
        let res = data.execute(&req_ctx.app.graphql_schema, &req_ctx);
        Ok::<_, serde_json::error::Error>((serde_json::to_string(&res)?, req_ctx))
    }).await?;

    let new_identity = context.identity.replace(Some(String::new()));
    if new_identity != original_identity {
        match new_identity {
            None => identity.forget(),
            Some(x) => identity.remember(x),
        }
    }

    let final_coins = context.get_quota_coins();
    if req_quota != final_coins {
        if let (Some(bank), Some(user)) = (&context.app.quota_bank, context.raw_user_id()) {
            let coin_diff = final_coins - req_quota;
            bank.add_quota_balance(Instant::now(), user, coin_diff)
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
