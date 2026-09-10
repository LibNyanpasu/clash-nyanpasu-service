use chrono::{DateTime, Utc};
/// Wall-clock adapter. Interval deadlines use Tokio's monotonic clock instead.
pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> DateTime<Utc>;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
