use actix_web::{http::StatusCode, ResponseError, web::HttpResponse};
use derive_more::Display;
use diesel::result::{DatabaseErrorKind, Error as DBError};
use juniper::FieldError;
use mysql::Error as MySqlError;

#[derive(Debug, Display)]
pub enum ServiceError {
    #[display(fmt = "Internal Server Error: {}", _0)]
    InternalServerError(String),

    #[display(fmt = "Bad Request: {}", _0)]
    BadRequest(String),

    #[display(fmt = "{} Not Found", _0)]
    NotFound(String),

    #[display(fmt = "Unauthorized")]
    Unauthorized,

    #[display(fmt = "Wrong Password")]
    WrongPassword,

    #[display(fmt = "Login Required")]
    LoginRequired,

    #[display(fmt = "{} Already Present", _0)]
    AlreadyPresent(String),

    #[display(fmt = "Too Many Requests")]
    TooManyRequests,
}

impl juniper::IntoFieldError for ServiceError {
    fn into_field_error(self) -> FieldError {
        match self {
            // TODO: log InternalServerErrors
            ServiceError::InternalServerError(mex) => FieldError::new(
                "Internal server error",
                graphql_value!({
                    "type": "INTERNAL_SERVER_ERROR",
                    "info": mex
                })
            ),
            ServiceError::BadRequest(message) => FieldError::new(
                message,
                graphql_value!({
                    "type": "BAD_REQUEST"
                })
            ),
            ServiceError::NotFound(type_name) => FieldError::new(
                format!("{} not found!", type_name),
                graphql_value!({
                    "type": "NOT_FOUND"
                })
            ),
            ServiceError::Unauthorized => FieldError::new(
                "Higher authorization required",
                graphql_value!({
                    "type": "UNAUTHORIZED"
                })
            ),
            ServiceError::WrongPassword => FieldError::new(
                "Wrong password",
                graphql_value!({
                    "type": "WRONG_PASSWORD"
                })
            ),
            ServiceError::LoginRequired => FieldError::new(
                "Login required",
                graphql_value!({
                    "type": "LOGIN_REQUIRED"
                })
            ),
            ServiceError::AlreadyPresent(type_name) => FieldError::new(
                format!("{} already taken", type_name),
                graphql_value!({
                    "type": "ALREADY_PRESENT"
                })
            ),
            ServiceError::TooManyRequests => FieldError::new(
                "Too many requests",
                graphql_value!({
                    "type": "TOO_MANY_REQUESTS"
                })
            )
        }
    }
}

impl From<DBError> for ServiceError {
    fn from(error: DBError) -> ServiceError {
        match error {
            DBError::DatabaseError(kind, info) => {
                let message = info.details().unwrap_or_else(|| info.message()).to_string();
                if let DatabaseErrorKind::UniqueViolation = kind {
                    ServiceError::AlreadyPresent(message)
                } else {
                    ServiceError::InternalServerError(format!("DB error, {:?} {:?}", kind, info))
                }
            }
            err => ServiceError::InternalServerError(format!("DB error, {}", err)),
        }
    }
}

impl From<r2d2::Error> for ServiceError {
    fn from(error: r2d2::Error) -> ServiceError {
        ServiceError::InternalServerError(format!("Pool error: {}", error))
    }
}

impl From<MySqlError> for ServiceError {
    fn from(error: MySqlError) -> ServiceError {
        ServiceError::InternalServerError(format!("MySql Error: {}", error))
    }
}

impl ResponseError for ServiceError {
    fn error_response(&self) -> HttpResponse {
        match self {
            ServiceError::InternalServerError(x) => HttpResponse::InternalServerError().message_body(x.into()),
            ServiceError::BadRequest(x) => HttpResponse::BadRequest().message_body(x.into()),
            ServiceError::NotFound(x) => HttpResponse::NotFound().message_body(format!("{} Not Found", x).into()),
            ServiceError::Unauthorized => HttpResponse::new(StatusCode::FORBIDDEN),
            ServiceError::WrongPassword => HttpResponse::Unauthorized().message_body("Wrong Password".into()),
            ServiceError::LoginRequired => HttpResponse::Unauthorized().message_body("Login required".into()),
            ServiceError::AlreadyPresent(x) => HttpResponse::BadRequest().message_body(format!("{} Already Present", x).into()),
            ServiceError::TooManyRequests => HttpResponse::new(StatusCode::TOO_MANY_REQUESTS),
        }
    }
}

pub type ServiceResult<T> = Result<T, ServiceError>;
