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
}

pub fn auto_create_site(site_id: IdType, cnr_id: &str, conn: &PgConnection, mysql_conn: &mysql::Pool) -> ServiceResult<()> {
    use crate::schema::sensor::dsl as sensor_dsl;
    use crate::schema::channel::dsl as channel_dsl;

    let res = mysql_conn.prep_exec("SELECT DISTINCT idsensore, canale FROM (SELECT * FROM t_rilevamento_dati WHERE idsito = :site_id ORDER BY data DESC LIMIT 1000) AS tmp;", params!{
        "site_id" => cnr_id
    })?;

    let mut sensor_to_channel: HashMap<String, Vec<String>> = HashMap::new();

    for row in res {
        let (sensor_id, channel_cnr_id) = mysql::from_row::<(String, String)>(row.unwrap());
        sensor_to_channel.entry(sensor_id).or_insert_with(Vec::new).push(channel_cnr_id)
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
            let data = AutoChannelData {
                sensor_id: id,
                id_cnr: Some(x.clone()),
                name: Some(x),
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

    let res = mysql_conn.prep_exec("SELECT DISTINCT canale FROM (SELECT * FROM t_rilevamento_dati WHERE idsito = :site_id AND idsensore = :sensor_id ORDER BY data DESC LIMIT 100) AS tmp;", params!{
        "site_id" => site_cnr_id,
        "sensor_id" => cnr_id,
    })?;

    let channels: Vec<AutoChannelData> = res.map(|row| {
        let cnr_id = mysql::from_row::<String>(row.unwrap());

        AutoChannelData {
            sensor_id,
            id_cnr: Some(cnr_id.clone()),
            name: Some(cnr_id),
        }
    }).collect();

    diesel::insert_into(channel_dsl::channel)
        .values(&channels)
        .execute(conn)?;

    Ok(())
}
