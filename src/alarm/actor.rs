use std::time::{Duration, Instant};

use actix::prelude::*;
use log::{error, info};

use crate::AppData;

use super::controller::check_measures;

pub struct AlarmActor {
    pub app_data: AppData,
}

impl AlarmActor {
    fn on_tick(&mut self, _ctx: &mut Context<Self>) {
        let start = Instant::now();

        let sensor_pool = &self.app_data.sensor_pool;
        let connection = self.app_data.pool.get();

        let connection = match connection {
            Ok(x) => x,
            Err(desc) => {
                error!("Error in connection pool: {}", desc);
                return
            },
        };

        if let Err(description) = check_measures(&connection, sensor_pool) {
            error!("Error during measurement check: {}", description);
        }

        info!("Measurement checked in {}ms", start.elapsed().as_millis());
    }
}

impl Actor for AlarmActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        info!("starting the alarm actor");

        IntervalFunc::new(Duration::from_millis(60000), Self::on_tick)
            .finish()
            .spawn(ctx);
    }
}
