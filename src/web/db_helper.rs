use std::collections::HashMap;

use diesel::{PgConnection, prelude::*};
use mysql::params;

use crate::models::IdType;
use crate::schema::*;
use crate::web::errors::ServiceResult;

#[derive(juniper::GraphQLInputObject, Insertable, AsChangeset)]
#[table_name="sensor"]
struct AutoSensorData {
    pub site_id: IdType,
    pub id_cnr: Option<String>,

    pub name: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(juniper::GraphQLInputObject, Insertable, AsChangeset)]
#[table_name="channel"]
struct AutoChannelData {
    pub sensor_id: IdType,
    pub id_cnr: Option<String>,

    pub name: Option<String>,
    pub measure_unit: Option<String>,
}

struct ChannelDetectedData {
    pub measure_unit: String,
    pub name: Option<String>,
}

fn guess_channel_info(m_type: &str) -> ChannelDetectedData {
    if m_type.starts_with('T') {
        let name = if m_type.starts_with("TSUP") {
            Some("T. Superfice".to_string())
        } else if m_type.starts_with("T_RUG") {
            Some("T. Rugiada".to_string())
        } else {
            Some("Temperatura".to_string())
        };
        ChannelDetectedData {
            measure_unit: "C°".to_string(),
            name,
        }
    } else if m_type.starts_with("COND") {
        ChannelDetectedData {
            measure_unit: "C°".to_string(),
            name: Some("T. Condensa".to_string()),
        }
    } else if m_type.starts_with("UR") {
        ChannelDetectedData {
            measure_unit: "%".to_string(),
            name: Some("Umidità Relativa".to_string()),
        }
    } else if m_type.starts_with("RELAY") {
        ChannelDetectedData {
            measure_unit: "y/n".to_string(),
            name: Some("Relay".to_string()),
        }
    } else if m_type.starts_with("CO2") {
        ChannelDetectedData {
            measure_unit: "PPM".to_string(),
            name: Some("CO2".to_string()),
        }
    } else {
        // Guessing failed
        ChannelDetectedData {
            measure_unit: m_type.to_string(),
            name: None,
        }
    }
}

pub fn auto_create_site(site_id: IdType, cnr_id: &str, conn: &PgConnection, mysql_conn: &mysql::Pool) -> ServiceResult<()> {
    use crate::schema::sensor::dsl as sensor_dsl;
    use crate::schema::channel::dsl as channel_dsl;

    let res = mysql_conn.prep_exec("SELECT DISTINCT idsensore, canale, misura FROM (SELECT * FROM t_rilevamento_dati WHERE idsito = :site_id ORDER BY data DESC LIMIT 1000) AS tmp;", params!{
        "site_id" => cnr_id
    })?;

    struct ChannelData {
        cnr_id: String,
        measure_type: String,
    }

    let mut sensor_to_channel: HashMap<String, Vec<ChannelData>> = HashMap::new();

    for row in res {
        let (sensor_id, channel_cnr_id, channel_measure) = mysql::from_row::<(String, String, String)>(row.unwrap());
        sensor_to_channel.entry(sensor_id).or_insert_with(Vec::new).push(
            ChannelData {
                cnr_id: channel_cnr_id,
                measure_type: channel_measure,
            }
        )
    }

    for (id_cnr, mut channels) in sensor_to_channel.drain() {
        let data = AutoSensorData {
            site_id,
            id_cnr: Some(id_cnr.clone()),
            name: Some(id_cnr),
            enabled: Some(true),
        };
        let id = diesel::insert_into(sensor_dsl::sensor)
            .values(&data)
            .returning(sensor_dsl::id)
            .get_result(conn)?;

        for x in channels.drain(..) {
            let info = guess_channel_info(x.measure_type.as_str());

            let data = AutoChannelData {
                sensor_id: id,
                id_cnr: Some(x.cnr_id.clone()),
                name: Some(info.name.unwrap_or(x.cnr_id)),
                measure_unit: Some(info.measure_unit),
            };
            diesel::insert_into(channel_dsl::channel)
                .values(&data)
                .execute(conn)?;
        }
    }

    Ok(())
}

pub fn auto_create_sensor(site_cnr_id: &str, sensor_id: IdType, cnr_id: &str, conn: &PgConnection, mysql_conn: &mysql::Pool) -> ServiceResult<()> {
    use crate::schema::channel::dsl as channel_dsl;

    let res = mysql_conn.prep_exec("SELECT DISTINCT canale, misura FROM (SELECT * FROM t_rilevamento_dati WHERE idsito = :site_id AND idsensore = :sensor_id ORDER BY data DESC LIMIT 100) AS tmp;", params!{
        "site_id" => site_cnr_id,
        "sensor_id" => cnr_id,
    })?;

    let channels: Vec<AutoChannelData> = res.map(|row| {
        let (cnr_id, measure_type) = mysql::from_row::<(String, String)>(row.unwrap());

        let info = guess_channel_info(measure_type.as_str());

        AutoChannelData {
            sensor_id,
            id_cnr: Some(cnr_id.clone()),
            name: Some(info.name.unwrap_or(cnr_id)),
            measure_unit: Some(info.measure_unit),
        }
    }).collect();

    diesel::insert_into(channel_dsl::channel)
        .values(&channels)
        .execute(conn)?;

    Ok(())
}
