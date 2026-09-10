use crate::model::*;
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Synchronous storage port. Called only by the owned, single blocking worker.
/// `admit`, `finish`, and `prune` must be atomic; retries of finish are idempotent.
/// `AdmissionFailed` guarantees no record was committed; other admission errors
/// are uncertain and will be resolved by reading the same ID, never by resubmitting.
/// Do not retain references to application actors or perform business effects here.
pub trait JobStore: Send + 'static {
    fn recover(&mut self, now: DateTime<Utc>) -> Result<(), Error>;
    fn admit(&mut self, record: RunRecord) -> Result<RunRecord, Error>;
    fn append(&mut self, id: RunId, logs: &[RunLog]) -> Result<(), Error>;
    fn finish(&mut self, record: &RunRecord, logs: &[RunLog]) -> Result<(), Error>;
    fn get(&mut self, id: RunId) -> Result<Option<RunRecord>, Error>;
    fn runs(
        &mut self,
        job: &str,
        after: Option<RunCursor>,
        limit: usize,
    ) -> Result<Page<RunRecord, RunCursor>, Error>;
    fn logs(&mut self, id: RunId, after: u64, limit: usize) -> Result<Page<RunLog, u64>, Error>;
    fn prune(&mut self, now: DateTime<Utc>, retention: &Retention) -> Result<(), Error>;
    fn health(&mut self) -> Result<(), Error>;
}

type StoreOperation = Box<dyn FnOnce(&mut dyn JobStore) + Send>;
enum Work {
    Call(StoreOperation),
    Stop,
}
#[derive(Clone)]
pub(crate) struct Journal {
    sender: mpsc::Sender<Work>,
}
pub(crate) struct JournalWorker {
    pub client: Journal,
    pub task: JoinHandle<()>,
}
impl JournalWorker {
    pub fn start(mut store: Box<dyn JobStore>) -> Self {
        let (sender, mut receiver) = mpsc::channel::<Work>(128);
        // Narrow infrastructure worker: one owned blocking thread, bounded queue,
        // and explicit join. No actor-owned state is shared with the adapter.
        let task = tokio::task::spawn_blocking(move || {
            while let Some(work) = receiver.blocking_recv() {
                match work {
                    Work::Call(work) => work(store.as_mut()),
                    Work::Stop => break,
                }
            }
        });
        Self {
            client: Journal { sender },
            task,
        }
    }
}
impl Journal {
    pub async fn close(&self) {
        let _ = self.sender.send(Work::Stop).await;
    }
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut dyn JobStore) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let (send, recv) = oneshot::channel();
        self.sender
            .send(Work::Call(Box::new(move |store| {
                let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(store)))
                    .unwrap_or_else(|_| Err(Error::Journal("store_panicked".into())));
                let _ = send.send(value);
            })))
            .await
            .map_err(|_| Error::Stopped)?;
        recv.await.map_err(|_| Error::Stopped)?
    }
    pub async fn query<T: Send + 'static>(
        &self,
        timeout: Duration,
        f: impl FnOnce(&mut dyn JobStore) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        tokio::time::timeout(timeout, self.call(f))
            .await
            .map_err(|_| Error::TimedOut)?
    }
}
