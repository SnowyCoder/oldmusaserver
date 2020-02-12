use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::AsRef;
use std::ops::DerefMut;
use std::rc::Rc;
use std::sync::Mutex;

use actix_http::cookie::CookieJar;
use actix_http::Request;
use actix_identity::{CookieIdentityPolicy, IdentityService};
use actix_web::{App, test};
use actix_web::dev::{PayloadStream, Service, ServiceResponse};
use actix_web::http::header;
use juniper::DefaultScalarValue;
use juniper::http::GraphQLRequest;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;

use oldmusa_server::*;
use futures::executor::block_on;

lazy_static! {
    static ref MIGRATION_SETUP: Mutex<()> = Mutex::new(());
    static ref ROOT_PASSWORD: Mutex<RefCell<Option<CookieJar>>> = Mutex::new(RefCell::new(None));
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ExecutionError {
    pub locations: Value,
    pub path: Option<Vec<String>>,
    pub message: String,
    pub extensions: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
pub struct GraphQLResult {
    pub data: Option<Value>,
    pub errors: Option<Vec<ExecutionError>>,
}

pub trait ExecutionErrorContainer {
    fn expect_service_error(&self, error_type: &str);
}

impl<T> ExecutionErrorContainer for Result<T, Vec<ExecutionError>> {
    fn expect_service_error(&self, error_type: &str) {
        let errors = self.as_ref().err().expect("Expected errored result");

        if !errors.iter().any(|x| {
            x.extensions.as_ref().map(|x| { x["type"] == error_type}).unwrap_or(false)
        }) {
            panic!(format!("Cannot find error: {} in {:?}", error_type, errors))
        }
    }
}

pub struct GraphQlQueryBuilder {
    query: String,
    variables: HashMap<String, Value>,
    operation_name: Option<String>,
}

impl GraphQlQueryBuilder {
    pub fn query<S: Into<String>>(query: S) -> GraphQlQueryBuilder {
        GraphQlQueryBuilder {
            query: query.into(),
            variables: HashMap::new(),
            operation_name: None
        }
    }

    pub fn add_variable<S: Into<String>, V: Into<Value>>(mut self, name: S, value: V) -> Self {
        self.variables.insert(name.into(), value.into());
        self
    }
}

pub fn query<S: Into<String>>(query: S) -> GraphQlQueryBuilder {
    GraphQlQueryBuilder::query(query)
}

impl Into<GraphQLRequest> for GraphQlQueryBuilder {
    fn into(self) -> GraphQLRequest<DefaultScalarValue> {
        GraphQLRequest::new(
            self.query,
            self.operation_name,
            Some(serde_json::from_str(&serde_json::to_string(&self.variables).unwrap()).unwrap())
        )
    }
}

fn json_object_extract_first(val: &Value) -> Option<Value> {
    match val.as_object().and_then(|x| x.values().next()) {
        Some(x) => return Some(x.clone()),
        None => None,
    }
}

pub fn create_random_username() -> String {
    let data = rand::thread_rng().gen::<[u8; 16]>();
    hex::encode(&data)
}

pub trait GraphQlTester : Clone {
    fn submit_raw<R: Into<GraphQLRequest>>(&mut self, query: R) -> Result<Value, Vec<ExecutionError>>;

    fn submit<R: Into<GraphQLRequest>>(&mut self, query: R) -> Value {
        let x = self.submit_raw(query);
        match x {
            Ok(val) => return json_object_extract_first(&val).expect("Cannot parse value"),
            Err(errors) => Self::manage_errors(errors),
        };
    }

    fn submit_all<R: Into<GraphQLRequest>>(&mut self, query: R) -> Value {
        let x = self.submit_raw(query);
        match x {
            Ok(val) => return val,
            Err(errors) => Self::manage_errors(errors),
        };
    }

    fn manage_errors(errors: Vec<ExecutionError>) -> ! {
        let errors = errors.iter()
            .map(|x| x.message.clone())
            .collect::<Vec<String>>()
            .join("\n");
        panic!(errors)
    }

    fn login(&mut self, username: &str, password: &str) {
        self.submit(
            query(r#"mutation login($auth: AuthInput!) { login(auth: $auth ) { id } }"#)
                .add_variable("auth", json!({
                    "username": username,
                    "password": password
                }))
        );
    }

    fn login_root(&mut self) {
        self.submit(query(r#"mutation { login(auth: {username: "root", password: "password" }) { id }}"#));
    }

    fn create_random_user(&mut self, password: &str) -> (i64, String) {
        let mut last_execution_error: Option<Vec<ExecutionError>> = None;
        for _ in 0..10 {
            let username = create_random_username();
            let res = self.submit_raw(query(r#"mutation addUser($auth: UserInput!) {
                addUser(data: $auth) { id }
            }"#).add_variable("auth", json!({
                "username": &username,
                "password": password,
                "permission": "USER",
            })));

            match res {
                Ok(x) => {
                    return (json_object_extract_first(&x).unwrap()["id"].to_i64(), username)
                },
                Err(errs) => last_execution_error = Some(errs),
            }
        }
        panic!(format!("Error creating user, tried 10 times, {:?}", last_execution_error));
    }
}

pub struct GraphQlTesterImpl<S, B, E>
    where S: Service<Request = actix_http::Request, Response = ServiceResponse<B>, Error = E>,
          B: actix_http::body::MessageBody + 'static,
          E: std::fmt::Debug,
{
    pub service: Rc<RefCell<S>>,
    pub data: AppData,
    pub cookies: CookieJar,
}

impl<S, B, E> GraphQlTester for GraphQlTesterImpl<S, B, E>
    where S: Service<Request = actix_http::Request, Response = ServiceResponse<B>, Error = E>,
          B: actix_http::body::MessageBody + 'static,
          E: std::fmt::Debug,
{
    fn submit_raw<R: Into<GraphQLRequest>>(&mut self, query: R) -> Result<Value, Vec<ExecutionError>> {
        exec_graphql_raw(self.service.borrow_mut().deref_mut(), &mut self.cookies, query)
    }

    fn login_root(&mut self) {
        let global_cookiejar = ROOT_PASSWORD.lock().unwrap();
        if let Some(jar) = (&*global_cookiejar).clone().into_inner() {
            self.cookies = jar.clone();
        } else {
            self.data.setup_root_password("password".to_string(), true).unwrap();
            self.submit(query(r#"mutation { login(auth: { username: "root", password: "password" }) { id } }"#));
            global_cookiejar.replace(Some(self.cookies.clone()));
        }
    }
}

impl<S, B, E> Clone for GraphQlTesterImpl<S, B, E>
    where S: Service<Request = actix_http::Request, Response = ServiceResponse<B>, Error = E>,
          B: actix_http::body::MessageBody + 'static,
          E: std::fmt::Debug,
{
    fn clone(&self) -> Self {
        GraphQlTesterImpl{
            service: self.service.clone(),
            data: self.data.clone(),
            cookies: self.cookies.clone(),
        }
    }
}

pub fn init_app() -> impl GraphQlTester {
    dotenv::dotenv().ok();
    let database_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set");
    let sensor_database_url = std::env::var("SENSOR_DATABASE_URL").expect("SENSOR_DATABASE_URL must be set");
    let data = AppData::new("a".repeat(32), database_url, sensor_database_url, contact::Contacter::new(None));

    {
        let _guard = MIGRATION_SETUP.lock().unwrap();
        data.setup_migrations().unwrap();
    }

    let service = block_on(test::init_service(
        App::new()
            .data(data.clone())
            .wrap(IdentityService::new(
                CookieIdentityPolicy::new(&[41; 32]) // <- create cookie identity policy
                    .name("auth-cookie")
                    .secure(false)))
            .configure(api_service::config)
    ));

    GraphQlTesterImpl {
        service: Rc::new(RefCell::new(service)),
        data,
        cookies: CookieJar::new(),
    }
}

fn graphql_request<R: Into<GraphQLRequest>>(request: R, cookies: &CookieJar) -> Request<PayloadStream> {
    let mut partial = test::TestRequest::post()
        .uri("/api/graphql")
        .header(header::CONTENT_TYPE, "application/json")
        .set_json(&request.into());

    for cookie in cookies.iter() {
        partial = partial.cookie(cookie.clone());
    }

    partial.to_request()
}

fn exec_graphql_raw<S, B, E, R>(app: &mut S, cookies: &mut CookieJar, req: R) -> Result<Value, Vec<ExecutionError>>
    where
        S: Service<Request = actix_http::Request, Response = ServiceResponse<B>, Error = E>,
        B: actix_http::body::MessageBody + 'static,
        E: std::fmt::Debug,
        R: Into<GraphQLRequest>
{
    let greq = graphql_request(req, cookies);

    let result = block_on(test::call_service(app, greq));
    for cookie in result.response().cookies() {
        cookies.add(cookie.into_owned())
    }
    let body = block_on(test::read_body(result));
    let str = std::str::from_utf8(body.as_ref()).unwrap().to_string();
    let res = serde_json::from_str::<GraphQLResult>(str.as_str()).unwrap();
    if res.errors.is_some() {
        return Err(res.errors.unwrap())
    }
    Ok(res.data.unwrap())
}

pub trait IntoPrimitive {
    fn to_i64(&self) -> i64;
    fn to_u64(&self) -> u64;
    fn to_f64(&self) -> f64;
    fn to_bool(&self) -> bool;
    fn to_str(&self) -> &str;
}

impl IntoPrimitive for Value {
    fn to_i64(&self) -> i64 {
        self.as_i64().expect("Value is not i64")
    }

    fn to_u64(&self) -> u64 {
        self.as_u64().expect("Value is not u64")
    }

    fn to_f64(&self) -> f64 {
        self.as_f64().expect("Value is not f64")
    }

    fn to_bool(&self) -> bool {
        self.as_bool().expect("Value is not bool")
    }

    fn to_str(&self) -> &str {
        self.as_str().expect("Value is not string")
    }
}

pub fn assert_eq_set(mut left: Value, mut right: Value) {
    let left = left.as_array_mut().expect("Left is not array");
    let right = right.as_array_mut().expect("Right is not array");

    assert_eq!(left.len(), right.len());
    left.sort_by_cached_key(|x| format!("{}", x));
    right.sort_by_cached_key(|x| format!("{}", x));
    assert_eq!(left, right)
}
