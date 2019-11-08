use super::schema::*;
use diesel::{r2d2::ConnectionManager, MysqlConnection};
use serde::*;


// type alias to use in multiple places
pub type Pool = r2d2::Pool<ConnectionManager<MysqlConnection>>;

#[derive(Debug, Serialize, Deserialize, Queryable)]
pub struct SensorData {
    pub idsito: String,
    pub idstanza: String,
    pub idstazione: String,
    pub idsensore: String,
    pub canale: String,
    pub valore_min: f64,
    pub valore_med: Option<f64>,
    pub valore_max: Option<f64>,
    pub scarto: Option<f64>,
    pub data: chrono::NaiveDateTime,
    pub errore: Option<char>,
    pub misura: String,
    pub step: Option<f32>,
}
