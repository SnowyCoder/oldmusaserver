use core::fmt::{Display, Error as FormatError, Formatter};
use std::collections::HashMap;
use std::error::Error;
use std::string::ToString;

use bigdecimal::{BigDecimal, ToPrimitive};
use chrono::prelude::*;
use diesel::{
    pg::PgConnection,
    pg::upsert::*,
    prelude::*,
    result::Error as DieselError,
};
use futures::future::join_all;
use futures::prelude::*;
use log::{debug, warn};
use mysql::error::Error as MysqlError;
use mysql::error::Result as MysqlResult;
use mysql::params;

use crate::contact::{
    Contacter, MeasureExtremeType
};
use crate::models::IdType;
use crate::schema::site;

type Connection = PgConnection;

/// Loads the last measure in a channel using the chronological order, returning min_measure, max_measure, timestamp
/// The channel must be specified fully by the site, the sensor and the channel ids.
pub fn load_last_channel_measure(site_id: &str, sensor_id: &str, channel_id: &str, conn: &mysql::Pool) -> MysqlResult<(f64, f64, NaiveDateTime)> {
    let mut result = conn.prep_exec(
        "SELECT valore_min, valore_max, data FROM t_rilevamento_dati WHERE idsito = :site_id AND idsensore = :sensor_id AND canale = :channel_id ORDER BY data DESC LIMIT 1;",
        params!{
            "site_id" => site_id,
            "sensor_id" => sensor_id,
            "channel_id" => channel_id
        }
    )?;
    let data = mysql::from_row::<(f64, f64, NaiveDateTime)>(result.next().expect("Row expected, empty table found")?);
    Ok(data)
}

/// Loads the last measure of the site (among every channel)
/// # Panics
/// If the site has no measures (this should be revisited but it should never happen).
pub fn load_last_site_measure(site_id: &str, conn: &mysql::Pool) -> MysqlResult<(f64, f64, NaiveDateTime)> {
    let mut result = conn.prep_exec(
        "SELECT valore_min, valore_max, data FROM t_rilevamento_dati WHERE idsito = :site_id ORDER BY data DESC LIMIT 1;",
        params!{"site_id" => site_id}
    )?;
    let data = mysql::from_row::<(f64, f64, NaiveDateTime)>(result.next().expect("Row expected, empty table found")?);
    Ok(data)
}

#[derive(Debug)]
struct SiteData {
    pub min_value: f64,
    pub max_value: f64,
    pub sensor_id: String,
    pub channel_id: String,
}

/// Loads all of the measures that are newer than the clocks, and returns the minimum value and
/// the maximum value for every channel.
/// As the site id is provided as the parameter it is not returned.
fn load_channel_data(cnr_id: &str, clock: NaiveDateTime, conn: &mysql::Pool) -> MysqlResult<Vec<SiteData>> {
    let result = conn.prep_exec(
        "SELECT min(valore_min), max(valore_max), idsensore, canale FROM t_rilevamento_dati WHERE idsito = :site_id AND data > :clock GROUP BY idsito, idstazione, idsensore, canale;",
        params!{
            "site_id" => cnr_id,
            "clock" => clock
        }
    )?;
    let data: Vec<SiteData> = result.map(|row| {
        let (min_value, max_value, sensor_id, channel_id) =
            mysql::from_row::<(f64, f64, String, String)>(row.unwrap());
        SiteData { min_value, max_value, sensor_id, channel_id }
    }).collect();
    Ok(data)
}

#[derive(Debug, Queryable)]
pub struct SiteClockData(IdType, Option<String>, NaiveDateTime);

#[derive(Debug, Insertable)]
#[table_name = "site"]
pub struct SiteClockUpdateData {
    pub id: IdType,
    pub clock: chrono::NaiveDateTime,
}

/// Loads id, cnr_id and clock for every available site.
/// Sites without a cnr_id are not returned.
pub fn load_site_clocks(conn: &Connection) -> QueryResult<Vec<SiteClockData>> {
    use crate::schema::site::dsl::*;
    site.select((id, id_cnr, clock))
        .filter(id_cnr.is_not_null())
        .load::<SiteClockData>(conn)
}

/// Saves the sites clock data to the database (overriding the previous ones).
pub fn save_site_clocks(conn: &Connection, clocks: &[SiteClockUpdateData]) -> QueryResult<()>{
    use crate::schema::site::dsl::*;
    // Postgresql:
    // INSERT INTO tabelname(id, col2, col3, col4)
    //VALUES
    //    (1, 1, 1, 'text for col4'),
    //    (DEFAULT,1,4,'another text for col4')
    //ON CONFLICT (id) DO UPDATE SET
    //    col2 = EXCLUDED.col2,
    //    col3 = EXCLUDED.col3,
    //    col4 = EXCLUDED.col4

    // Mysql: INSERT INTO mytable (id, a, b, c)
    //VALUES (1, 'a1', 'b1', 'c1'),
    //(2, 'a2', 'b2', 'c2'),
    //(3, 'a3', 'b3', 'c3'),
    //(4, 'a4', 'b4', 'c4'),
    //(5, 'a5', 'b5', 'c5'),
    //(6, 'a6', 'b6', 'c6')
    //ON DUPLICATE KEY UPDATE id=VALUES(id),
    //a=VALUES(a),
    //b=VALUES(b),
    //c=VALUES(c);

    let updated = diesel::insert_into(site)
        .values(clocks)
        .on_conflict(id)
        .do_update().set(clock.eq(excluded(clock)))
        .execute(conn)?;

    if updated != clocks.len() {
        // Is someone else operating on the same database?
        warn!("Warning: {} clocks failed to update", clocks.len() - updated);
        // TODO: ?
    }
    Ok(())
}

#[derive(Queryable)]
struct ChannelAlarmDataRaw {
    site_id: IdType,
    sensor_id: IdType,
    sensor_cnr_id: Option<String>,
    channel_id: IdType,
    channel_cnr_id: Option<String>,
    range_min: Option<BigDecimal>,
    range_max: Option<BigDecimal>,
}

#[derive(Debug)]
struct ChannelAlarmData {
    site_id: IdType,
    sensor_id: IdType,
    sensor_cnr_id: String,
    channel_id: IdType,
    channel_cnr_id: String,
    range_min: f64,
    range_max: f64,
}

/// Loads all of the data related to alarms for every enabled channel.
/// The site cnr id isn't returned (as it is already present with the clock).
/// The sensors and channels that don't have the cnr_id are not returned.
/// If a channel doesn't have a min_value it is replaced with -inf, and if the
/// max_value is not present it is replaced with +inf.
fn load_channels_alarm_data(conn: &Connection) -> QueryResult<Vec<ChannelAlarmData>> {
    use crate::schema::site::dsl as site_dsl;
    use crate::schema::sensor::dsl as sensor_dsl;
    use crate::schema::channel::dsl as channel_dsl;

    let data = channel_dsl::channel
        .inner_join(sensor_dsl::sensor.inner_join(site_dsl::site))
        .filter(sensor_dsl::enabled.eq(true))
        .filter(channel_dsl::id_cnr.is_not_null())
        .filter(sensor_dsl::id_cnr.is_not_null())
        .select((site_dsl::id, sensor_dsl::id, sensor_dsl::id_cnr, channel_dsl::id, channel_dsl::id_cnr, channel_dsl::range_min, channel_dsl::range_max))
        .load::<ChannelAlarmDataRaw>(conn)?
        .iter()
        .map(|x| ChannelAlarmData {
            site_id: x.site_id,
            sensor_id: x.sensor_id,
            sensor_cnr_id: x.sensor_cnr_id.as_ref().map(|x| x.to_string()).unwrap_or_else(|| "".to_string()),
            channel_id: x.channel_id,
            channel_cnr_id: x.channel_cnr_id.as_ref().map(|x| x.to_string()).unwrap_or_else(||  "".to_string()),
            range_min: x.range_min.as_ref().and_then(|x| x.to_f64()).unwrap_or(std::f64::NEG_INFINITY),
            range_max: x.range_max.as_ref().and_then(|x| x.to_f64()).unwrap_or(std::f64::INFINITY),
        }).collect();
    Ok(data)
}

#[derive(Queryable)]
struct AlarmedChannelDataRaw {
    channel_id: IdType,
    site_cnr_id: Option<String>,
    sensor_cnr_id: Option<String>,
    channel_cnr_id: Option<String>,
    range_min: Option<BigDecimal>,
    range_max: Option<BigDecimal>,
}

struct AlarmedChannelData {
    channel_id: IdType,
    site_cnr_id: String,
    sensor_cnr_id: String,
    channel_cnr_id: String,
    range_min: f64,
    range_max: f64,
}

/// Loads the data for the alarmed channels.
fn load_alarmed_data(conn: &Connection) -> QueryResult<Vec<AlarmedChannelData>> {
    use crate::schema::site::dsl as site_dsl;
    use crate::schema::sensor::dsl as sensor_dsl;
    use crate::schema::channel::dsl as channel_dsl;

    Ok(channel_dsl::channel
        .inner_join(sensor_dsl::sensor.inner_join(site_dsl::site))
        .filter(channel_dsl::alarmed.eq(true))
        .select((channel_dsl::id, site_dsl::id_cnr, sensor_dsl::id_cnr, channel_dsl::id_cnr, channel_dsl::range_min, channel_dsl::range_max))
        .order_by(channel_dsl::id.asc())
        .load::<AlarmedChannelDataRaw>(conn)?
        .iter()
        .map(|x| AlarmedChannelData {
            channel_id: x.channel_id,
            site_cnr_id: x.site_cnr_id.as_ref().map(|x| x.to_string()).unwrap_or_else(|| "".to_string()),
            sensor_cnr_id: x.sensor_cnr_id.as_ref().map(|x| x.to_string()).unwrap_or_else(||  "".to_string()),
            channel_cnr_id: x.channel_cnr_id.as_ref().map(|x| x.to_string()).unwrap_or_else(|| "".to_string()),
            range_min: x.range_min.as_ref().and_then(|x| x.to_f64()).unwrap_or(std::f64::NEG_INFINITY),
            range_max: x.range_max.as_ref().and_then(|x| x.to_f64()).unwrap_or(std::f64::INFINITY),
        }).collect())
}

/// Main function, checks all of the new data and manages alarms.
///
/// Every site has its own clock for which the measure timestamps are checked against.
/// For each site the saved clock is queried, then the new measures are downloaded and checked for
/// alarms, finally the last measure is queried and its timestamp is used as the new site clock.
/// To save bandwidth we only download the minimum and the maximum measure for each channel, letting
/// the DBMS do the computations.
/// Then the alarmed channels are computed: for each alarmed channel the last measure found is
/// queried, then if its within the min-max range the alarm is terminated.
pub fn check_measures(contacter: &Contacter, conn: &Connection, pool: &mysql::Pool) -> Result<Box<dyn Future<Item = (), Error = ()>>, DatabaseError> {
    let mut started_futures = vec![];
    let clocks = load_site_clocks(conn)?;

    let mut clocks_data: Vec<(IdType, (f64, f64, NaiveDateTime))> = vec![];
    let mut channel_data: Vec<(IdType, String, Vec<SiteData>)> = vec![];
    let mut updated_clocks: Vec<SiteClockUpdateData> = vec![];
    updated_clocks.reserve(clocks.len());

    let alarmed_data: Vec<AlarmedChannelData> = load_alarmed_data(conn)?;

    for SiteClockData(site_id, cnr_id, clock) in clocks.iter() {
        let cnr_id = if let Some(x) = cnr_id { x } else { continue };

        let data = load_channel_data(cnr_id, *clock, pool)?;

        let last_measure = load_last_site_measure(cnr_id, pool)?;

        debug!(" checking: {} = {} ({:?})", site_id, cnr_id, data);

        clocks_data.push((*site_id, last_measure));
        updated_clocks.push(SiteClockUpdateData {
            id: *site_id,
            clock: last_measure.2,
        });
        channel_data.push((*site_id, cnr_id.to_string(), data));
    }
    save_site_clocks(conn, &updated_clocks)?;


    let channels_alarm_data = load_channels_alarm_data(conn)?;

    //println!(" alarm data: {:?}", channels_alarm_data);
    let params_to_alarm_data: HashMap<(IdType, &str, &str), &ChannelAlarmData> = channels_alarm_data.iter()
        .map(|x| (
            (x.site_id, x.sensor_cnr_id.as_str(), x.channel_cnr_id.as_str()),
            x
        ))
        .collect();

    for (site_id, _site_cnr_id, data) in channel_data {
        for channel_data in data {
            let alarm_data = params_to_alarm_data.get(&(site_id, &channel_data.sensor_id, &channel_data.channel_id));
            if let Some(alarm_data) = alarm_data {
                if channel_data.min_value < alarm_data.range_min || channel_data.max_value > alarm_data.range_max {
                    if let Err(_insert_index) = alarmed_data.binary_search_by_key(&alarm_data.channel_id, |x| { x.channel_id }) {
                        // New alarm found
                        // Should we insert it in the alarm_data? I don't think it is useful: we
                        // shouldn't check this alarm in this tick. Or we could have alarms that
                        // last 0 seconds.
                        /*alarmed_data.insert(insert_index, AlarmedChannelData {
                            channel_id: alarm_data.channel_id,
                            site_cnr_id: site_cnr_id.clone(),
                            sensor_cnr_id: alarm_data.sensor_cnr_id.clone(),
                            channel_cnr_id: alarm_data.channel_cnr_id.clone(),
                            range_min: alarm_data.range_min,
                            range_max: alarm_data.range_max,
                        });*/
                        let (measure, measure_type) = if channel_data.min_value < alarm_data.range_min {
                            (channel_data.min_value, MeasureExtremeType::Min)
                        } else {
                            (channel_data.max_value, MeasureExtremeType::Max)
                        };
                        let future = alarm_begin(contacter, conn, alarm_data.channel_id, measure, measure_type)?;
                        started_futures.push(future)
                    }
                }
            }
        }
    }

    for alarm in alarmed_data {
        // Alarm checks
        let (measure_min, measure_max,  _measure_time) = load_last_channel_measure(&alarm.site_cnr_id, &alarm.sensor_cnr_id, &alarm.channel_cnr_id, pool)?;

        if measure_min > alarm.range_min && measure_max < alarm.range_max {
            alarm_end(conn, alarm.channel_id)?;
        }
    }

    Ok(Box::new(join_all(started_futures).map(|_| {})))
}

fn alarm_begin(contacter: &Contacter, conn: &Connection, channel_id: IdType, measure: f64, measure_type: MeasureExtremeType) -> Result<Box<dyn Future<Item = (), Error = ()>>, DatabaseError> {
    use crate::schema::channel::dsl;
    warn!("alarm_begin({} {} {:?})", channel_id, measure, measure_type);

    diesel::update(dsl::channel.find(channel_id))
        .set(dsl::alarmed.eq(true))
        .execute(conn)?;

    let future = contacter.send_alarm(conn, channel_id, measure, measure_type)?;

    Ok(Box::new(future))
}

fn alarm_end(conn: &Connection, channel_id: IdType) -> QueryResult<()> {
    use crate::schema::channel::dsl;
    warn!("alarm_end({})", channel_id);

    diesel::update(dsl::channel.find(channel_id))
        .set(dsl::alarmed.eq(false))
        .execute(conn)?;

    // TODO: Reset fcm?

    Ok(())
}

#[derive(Debug)]
pub struct DatabaseError(pub String);

impl Display for DatabaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FormatError> {
        f.write_str(&self.0)?;
        Ok(())
    }
}

impl Error for DatabaseError {}


impl From<DieselError> for DatabaseError {
    fn from(error: DieselError) -> DatabaseError {
        DatabaseError(error.to_string())
    }
}

impl From<MysqlError> for DatabaseError {
    fn from(error: MysqlError) -> DatabaseError {
        DatabaseError(error.to_string())
    }
}

impl From<String> for DatabaseError {
    fn from(error: String) -> DatabaseError {
        DatabaseError(error)
    }
}
