use std::time::{Duration, Instant};

use actix::prelude::*;
use diesel::PgConnection;
use diesel::r2d2::ConnectionManager;
use log::{error, info};
use r2d2::PooledConnection;

use crate::AppData;
use crate::contact::Contacter;

use super::controller::check_measures;

pub struct AlarmActor {
    pub app_data: AppData,
    pub sleep_interval: Duration,
}

impl AlarmActor {

    async fn on_tick_async2(
        start: Instant,
        contacter: Contacter,
        connection: PooledConnection<ConnectionManager<PgConnection>>,
        sensor_pool: mysql::Pool
    ) {
        let res = check_measures(&contacter, &connection, &sensor_pool).await;
        match res {
            Ok(()) => {},
            Err(description) => error!("Error during measurement check: {}", description),
        }
        info!("Measurement checked in {}ms", start.elapsed().as_millis());
    }

    fn on_tick_async(&mut self) -> Option<impl Future<Output=()>> {
        let start = Instant::now();

        let sensor_pool = self.app_data.sensor_pool.clone();
        let connection = self.app_data.pool.get();

        let connection = match connection {
            Ok(x) => x,
            Err(desc) => {
                error!("Error in connection pool: {}", desc);
                return None
            },
        };

        let mes_result = Self::on_tick_async2(start, self.app_data.contacter.clone(), connection, sensor_pool);

        Some(mes_result)
    }

    fn on_tick(&mut self, ctx: &mut Context<Self>) {
        let data = self.on_tick_async();
        if let Some(data) = data {
            ctx.spawn(data.into_actor(self));
        }
    }
}

impl Actor for AlarmActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        info!("starting the alarm actor");

        IntervalFunc::new(self.sleep_interval, Self::on_tick)
            .finish()
            .spawn(ctx);

        self.on_tick(ctx);
    }
}
