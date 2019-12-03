use std::sync::Arc;

use diesel::PgConnection;
use diesel::prelude::*;
use futures::Future;
use futures::future::join_all;
use log::{debug, error, warn};

use crate::models::IdType;

use super::fcm::FcmContacter;

pub type DbConnection = PgConnection;

#[derive(Debug)]
pub enum MeasureExtremeType {
    Min, Max
}

#[derive(Debug)]
pub struct SensorRangeAlarmData {
    pub site_id: IdType,
    pub site_name: String,
    pub sensor_name: String,
    pub channel_name: String,
    pub value: String,
}

#[derive(Clone)]
pub struct Contacter {
    fcm_client: Option<Arc<FcmContacter>>,
}

impl Contacter {
    pub fn new(fcm_key: Option<String>) -> Self {
        Contacter {
            fcm_client: fcm_key.map(|x| Arc::new(FcmContacter::new(x)))
        }
    }

    pub fn new_from_env() -> Self {
        let fcm_api_key = std::env::var("FCM_API_KEY").ok();

        if fcm_api_key.is_none() {
            warn!("No FCM apy key found, disabling");
        }

        Self::new(fcm_api_key)
    }

    pub fn send_alarm(&self, conn: &DbConnection, channel_id: IdType, measure: f64, _measure_type: MeasureExtremeType) -> Result<impl Future<Item = (), Error = ()>, String> {
        use crate::schema::{
            channel::dsl as channel_dsl,
            sensor::dsl as sensor_dsl,
            site::dsl as site_dsl,
        };

        let data = channel_dsl::channel.find(channel_id)
            .inner_join(sensor_dsl::sensor.inner_join(site_dsl::site))
            .select((site_dsl::id, site_dsl::name, sensor_dsl::name, channel_dsl::name, channel_dsl::measure_unit))
            .get_result::<(IdType, Option<String>, Option<String>, Option<String>, Option<String>)>(conn)
            .map_err(|x| x.to_string())?;

        let payload = SensorRangeAlarmData {
            site_id: data.0,
            site_name: data.1.unwrap_or("?".to_string()),
            sensor_name: data.2.unwrap_or("?".to_string()),
            channel_name: data.3.unwrap_or("?".to_string()),
            value: format!("{} {}", measure, data.4.unwrap_or("".to_string()))
        };

        let mut futures = Vec::new();

        if let Some(fcm) = self.fcm_client.as_ref() {
            futures.push(Box::new(fcm.send_alarm(conn, &payload)?));
        } else {
            warn!("FCM disabled, skipping alarm notification")
        }

        Ok(join_all(futures).map(|_| {}).map_err(|_| {}))
    }
}




