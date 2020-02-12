use std::collections::HashSet;

use diesel::prelude::*;
use fcm::MessageBuilder;
use log::info;
use serde::Serialize;

use crate::models::{IdType, PermissionType};

use super::contacter::DbConnection;
use super::contacter::SensorRangeAlarmData;

const FCM_MAX_RECIPIENTS: u32 = 1000;

pub struct FcmContacter {
    fcm_client: fcm::Client,
    api_key: String,
}

impl FcmContacter {
    pub fn new(api_key: String) -> Self {
        FcmContacter {
            fcm_client: fcm::Client::new(),
            api_key
        }
    }

    fn get_fcm_site_receivers(&self, conn: &DbConnection, site_id: IdType) -> Result<Vec<String>, String> {
        use crate::schema::{
            user_account::dsl as user_dsl,
            fcm_user_contact::dsl as fcm_dsl,
            user_access::dsl as user_access_dsl,
        };

        let mut users: Vec<String> = user_access_dsl::user_access.inner_join(user_dsl::user_account.inner_join(fcm_dsl::fcm_user_contact))
            .filter(user_access_dsl::site_id.eq(site_id))
            .select(fcm_dsl::registration_id)
            .distinct()
            .order_by(fcm_dsl::registration_id.asc())
            .load::<String>(conn)
            .map_err(|x| x.to_string())?;

        let mut admins: Vec<String> = user_dsl::user_account.inner_join(fcm_dsl::fcm_user_contact)
            .filter(user_dsl::permission.eq(PermissionType::Admin.to_char()))
            .select(fcm_dsl::registration_id)
            .distinct()
            .order_by(fcm_dsl::registration_id.asc())
            .load::<String>(conn)
            .map_err(|x| x.to_string())?;

        let mut res: HashSet<String> = users.drain(..).chain(admins.drain(..)).collect();

        Ok(res.drain().collect())
    }

    pub async fn send_alarm(&self, conn: &DbConnection, data: &SensorRangeAlarmData) -> Result<(), String> {
        let payload = SensorRangeAlarmMessagePayload {
            mex_type: "sensor_range_alarm".to_string(),
            site_name: data.site_name.to_string(),
            sensor_name: data.sensor_name.to_string(),
            channel_name: data.channel_name.to_string(),
            value: data.value.to_string(),
        };

        let contacted = self.get_fcm_site_receivers(conn, data.site_id)?;

        self.send_message(&payload, contacted).await;
        Ok(())
    }

    pub async fn send_message<T: Serialize>(&self, message: &T, ids: Vec<String>) {
        for id_chunks in ids.chunks(FCM_MAX_RECIPIENTS as usize) {
            let mut builder = MessageBuilder::new_multi(&self.api_key, id_chunks);
            builder.data(message).unwrap();
            let message = builder.finalize();

            if let Err(err) = self.fcm_client.send(message).await {
                info!("Error sending alarm: {:?}", err);
            }
        }
    }
}

#[derive(Debug, Serialize)]
struct SensorRangeAlarmMessagePayload {
    #[serde(rename="type")]
    mex_type: String,
    site_name: String,
    sensor_name: String,
    channel_name: String,
    value: String,
}
