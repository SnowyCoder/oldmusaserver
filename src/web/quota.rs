use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex};

use crate::models::IdType;
use actix::{Actor, Context, Message, Handler, AsyncContext, SpawnHandle, Addr};
use std::time::{Instant, Duration};
use priority_queue::PriorityQueue;

#[inline]
fn get_accumulated_balance(passed: Duration, balance_per_second: u128) -> u128 {
    // Hope it doesn't overflow with u128...
    (passed.as_millis() * balance_per_second) / 1000
}

#[inline]
fn get_balance_wait(bal: u128, balance_per_sec: u128) -> Duration {
    // inverse of get_accumulated_balance, how much should I wait to accumulate bal?
    Duration::from_millis(((1000 * bal) / balance_per_sec) as u64)
}

pub struct QuotaControlActor {
    data: Arc<Mutex<Data>>,
    next_run: Option<SpawnHandle>,
}

impl QuotaControlActor {
    pub fn update(&mut self, ctx: &mut <Self as Actor>::Context) {
        let now = Instant::now();
        let data_ptr = self.data.clone();
        let mut data = data_ptr.lock().unwrap();

        while let Some((_user_id, date)) = data.next_expiration.peek() {
            let date = date.0;
            if date >= now {
                // Add one second to remove any rounding error
                let sleep_duration = date.duration_since(now) + Duration::from_secs(1);
                self.reschedule_after(ctx, sleep_duration);
                break
            }
            let (user_id, _date) = data.next_expiration.pop().unwrap();
            // Update the value (this is the same as add_balance(0) so it recomputes the balance)
            data.get_balance(now, user_id);
        }
    }

    pub fn reschedule_after(&mut self, ctx: &mut <Self as Actor>::Context, dur: Duration) {
        if let Some(handle) = self.next_run.take() {
            ctx.cancel_future(handle);
        }
        self.next_run = Some(ctx.notify_later(QuotaUpdateMessage(), dur));
    }
}

impl Actor for QuotaControlActor {
    type Context = Context<Self>;
}

impl Handler<QuotaUpdateMessage> for QuotaControlActor {
    type Result = ();

    fn handle(&mut self, _msg: QuotaUpdateMessage, ctx: &mut Self::Context) -> Self::Result {
        self.update(ctx);
    }
}

#[derive(Eq, PartialEq, Debug)]
struct ExpiryEntry {
    date: Instant,
    user: IdType,
    creation_timestamp: Instant,
}

impl PartialOrd for ExpiryEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.date.partial_cmp(&other.date).map(|x| x.reverse())
    }
}

impl Ord for ExpiryEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.date.cmp(&other.date).reverse()
    }
}

pub struct UserData {
    balance: i64,
    last_balance_update: Instant,
}

pub struct Data {
    max_balance: i64,
    balance_per_second: u64,
    users: HashMap<IdType, UserData>,
    next_expiration: PriorityQueue<IdType, Reverse<Instant>>
}

impl Data {
    pub fn new(max_balance: i64, balance_per_second: u64) -> Self {
        Data {
            max_balance,
            balance_per_second,
            users: HashMap::new(),
            next_expiration: PriorityQueue::with_capacity(256),
        }
    }

    pub fn get_balance(&mut self, now: Instant, user_id: IdType) -> i64 {
        self.add_balance(now, user_id, 0)
    }

    pub fn replace_balance(&mut self, now: Instant, user_id: IdType, new_balance: i64) -> i64 {
        if new_balance >= self.max_balance {
            self.users.remove(&user_id);
            return self.max_balance
        }

        self.users.insert(user_id, UserData {
            balance: new_balance,
            last_balance_update: now,
        });

        let wait_time = get_balance_wait((self.max_balance as i128 - new_balance as i128) as u128, self.balance_per_second as u128);
        self.next_expiration.push(user_id, Reverse(now + wait_time));

        new_balance
    }

    pub fn add_balance(&mut self, now: Instant, user_id: IdType, balance_diff: i64) -> i64 {
        let current_balance = match self.users.get_mut(&user_id) {
            Some(user) => {
                let passed_time = now.duration_since(user.last_balance_update);
                let added_bal = get_accumulated_balance(passed_time, self.balance_per_second as u128);
                user.balance as i128 + added_bal as i128
            },
            None => {
                self.max_balance as i128
            },
        };
        let new_balance = current_balance + balance_diff as i128;
        self.replace_balance(now, user_id, new_balance as i64)
    }
}

struct QuotaUpdateMessage();

impl Message for QuotaUpdateMessage {
    type Result = ();
}

#[derive(Clone)]
pub struct AppData {
    handle: Arc<Mutex<Data>>,
    actor_addr: Addr<QuotaControlActor>
}

impl AppData {
    pub fn get_quota_balance(&self, now: Instant, user_id: IdType) -> i64 {
        let mut data = self.handle.lock().unwrap();
        data.get_balance(now, user_id)
    }

    pub fn set_quota_balance(&self, now: Instant, user_id: IdType, balance: i64) {
        let mut data = self.handle.lock().unwrap();
        data.replace_balance(now, user_id, balance);
        self.actor_addr.do_send(QuotaUpdateMessage());
    }

    pub fn add_quota_balance(&self, now: Instant, user_id: IdType, balance_diff: i64) {
        let mut data = self.handle.lock().unwrap();
        data.add_balance(now, user_id, balance_diff);

        self.actor_addr.do_send(QuotaUpdateMessage());
    }
}

pub fn init(max_balance: i64, balance_per_second: u64) -> AppData {
    let data = Data::new(max_balance, balance_per_second);
    let data_arc = Arc::new(Mutex::new(data));

    let actor = QuotaControlActor {
        data: data_arc.clone(),
        next_run: None,
    };
    let addr = actor.start();

    AppData {
        handle: data_arc,
        actor_addr: addr,
    }
}


