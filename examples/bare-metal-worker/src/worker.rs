//! The in-process job queue and the workers that run it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tracing::{info, warn};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const MAX_WAIT_SECONDS: u64 = 24 * 60 * 60;
const MAX_PRIMES_LIMIT: u64 = 10_000_000;
const RETAINED_JOBS: usize = 1_000;
const LISTED_JOBS: usize = 50;

/// What a job does. The body of `POST /jobs` is one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case", deny_unknown_fields)]
pub enum Task {
    /// Occupy a worker for `seconds`, like a long export or a model run.
    Wait { seconds: u64 },
    /// Count the primes up to `limit`, which keeps one CPU busy.
    Primes { limit: u64 },
}

impl Task {
    fn validate(&self) -> Result<(), String> {
        match self {
            Task::Wait { seconds } if (1..=MAX_WAIT_SECONDS).contains(seconds) => Ok(()),
            Task::Wait { .. } => Err(format!("seconds must be 1 to {MAX_WAIT_SECONDS}")),
            Task::Primes { limit } if (2..=MAX_PRIMES_LIMIT).contains(limit) => Ok(()),
            Task::Primes { .. } => Err(format!("limit must be 2 to {MAX_PRIMES_LIMIT}")),
        }
    }

    fn describe(&self) -> String {
        match self {
            Task::Wait { seconds } => format!("wait {seconds}s"),
            Task::Primes { limit } => format!("primes up to {limit}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Job {
    pub id: String,
    #[serde(flatten)]
    pub task: Task,
    pub status: Status,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    /// The worker version that ran, or will run, the job.
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// The worker is shutting down and takes no new jobs.
    Draining,
    /// Too many jobs are waiting.
    QueueFull,
    Invalid(String),
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub concurrency: usize,
    pub queue_capacity: usize,
}

struct Inner {
    jobs: BTreeMap<u64, Job>,
    next_id: u64,
    /// Dropped by `drain`, which is what stops new jobs and lets the workers finish.
    queue: Option<mpsc::Sender<u64>>,
}

pub struct Worker {
    inner: Mutex<Inner>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl Worker {
    pub fn start(config: Config) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let worker = Arc::new(Worker {
            inner: Mutex::new(Inner {
                jobs: BTreeMap::new(),
                next_id: 1,
                queue: Some(sender),
            }),
            handles: Mutex::new(Vec::new()),
        });
        let receiver = Arc::new(AsyncMutex::new(receiver));
        let handles = (0..config.concurrency)
            .map(|_| tokio::spawn(worker.clone().run(receiver.clone())))
            .collect();
        *worker.handles.lock().expect("handles lock") = handles;
        worker
    }

    pub fn is_draining(&self) -> bool {
        self.inner().queue.is_none()
    }

    pub fn submit(&self, task: Task) -> Result<Job, SubmitError> {
        task.validate().map_err(SubmitError::Invalid)?;
        let mut guard = self.inner();
        let inner = &mut *guard;
        let queue = inner.queue.as_ref().ok_or(SubmitError::Draining)?;
        let permit = queue.try_reserve().map_err(|error| match error {
            mpsc::error::TrySendError::Full(()) => SubmitError::QueueFull,
            mpsc::error::TrySendError::Closed(()) => SubmitError::Draining,
        })?;

        let number = inner.next_id;
        inner.next_id += 1;
        let job = Job {
            id: format!("j-{number}"),
            task,
            status: Status::Queued,
            started_at: None,
            finished_at: None,
            version: VERSION,
            result: None,
            error: None,
        };
        inner.jobs.insert(number, job.clone());
        permit.send(number);
        forget_oldest_finished(&mut inner.jobs);
        Ok(job)
    }

    pub fn job(&self, id: &str) -> Option<Job> {
        let number = id.strip_prefix("j-")?.parse::<u64>().ok()?;
        self.inner().jobs.get(&number).cloned()
    }

    /// The most recent jobs, newest first.
    pub fn recent_jobs(&self) -> Vec<Job> {
        self.inner()
            .jobs
            .values()
            .rev()
            .take(LISTED_JOBS)
            .cloned()
            .collect()
    }

    /// Stops taking jobs, then returns once every accepted job has finished.
    pub async fn drain(&self) {
        let handles = {
            let mut inner = self.inner();
            inner.queue = None;
            let running = count(&inner.jobs, Status::Running);
            let queued = count(&inner.jobs, Status::Queued);
            let mut line = format!(
                "draining: {running} {} running",
                if running == 1 { "job" } else { "jobs" }
            );
            if queued > 0 {
                line.push_str(&format!(", {queued} queued"));
            }
            info!("{line}, new jobs refused");
            std::mem::take(&mut *self.handles.lock().expect("handles lock"))
        };
        for handle in handles {
            if let Err(error) = handle.await {
                warn!("worker task ended abnormally: {error}");
            }
        }
    }

    async fn run(self: Arc<Self>, queue: Arc<AsyncMutex<mpsc::Receiver<u64>>>) {
        loop {
            let next = queue.lock().await.recv().await;
            let Some(number) = next else { return };
            self.execute(number).await;
        }
    }

    async fn execute(&self, number: u64) {
        let (id, task) = {
            let mut inner = self.inner();
            let job = inner.jobs.get_mut(&number).expect("queued job exists");
            job.status = Status::Running;
            job.started_at = Some(Utc::now());
            (job.id.clone(), job.task.clone())
        };
        info!("job {id} started ({})", task.describe());

        let started = Instant::now();
        let outcome = run_task(&task).await;
        let seconds = started.elapsed().as_secs();

        let mut inner = self.inner();
        let job = inner.jobs.get_mut(&number).expect("running job exists");
        job.finished_at = Some(Utc::now());
        match outcome {
            Ok(result) => {
                job.status = Status::Finished;
                job.result = result;
                info!("job {id} finished in {seconds}s");
            }
            Err(error) => {
                info!("job {id} failed after {seconds}s: {error}");
                job.status = Status::Failed;
                job.error = Some(error);
            }
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().expect("worker state lock")
    }
}

async fn run_task(task: &Task) -> Result<Option<serde_json::Value>, String> {
    match task {
        Task::Wait { seconds } => {
            tokio::time::sleep(Duration::from_secs(*seconds)).await;
            Ok(None)
        }
        Task::Primes { limit } => {
            let limit = *limit;
            let primes = tokio::task::spawn_blocking(move || count_primes(limit))
                .await
                .map_err(|error| error.to_string())?;
            Ok(Some(serde_json::json!({ "primes": primes })))
        }
    }
}

fn count_primes(limit: u64) -> u64 {
    let limit = limit as usize;
    let mut composite = vec![false; limit + 1];
    let mut count = 0;
    for n in 2..=limit {
        if composite[n] {
            continue;
        }
        count += 1;
        for multiple in (n * n..=limit).step_by(n) {
            composite[multiple] = true;
        }
    }
    count
}

fn count(jobs: &BTreeMap<u64, Job>, status: Status) -> usize {
    jobs.values().filter(|job| job.status == status).count()
}

fn forget_oldest_finished(jobs: &mut BTreeMap<u64, Job>) {
    while jobs.len() > RETAINED_JOBS {
        let oldest_done = jobs
            .iter()
            .find(|(_, job)| matches!(job.status, Status::Finished | Status::Failed))
            .map(|(number, _)| *number);
        match oldest_done {
            Some(number) => jobs.remove(&number),
            None => return,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(concurrency: usize, queue_capacity: usize) -> Arc<Worker> {
        Worker::start(Config {
            concurrency,
            queue_capacity,
        })
    }

    fn wait(seconds: u64) -> Task {
        Task::Wait { seconds }
    }

    async fn until_status(worker: &Worker, id: &str, status: Status) {
        for _ in 0..1_000 {
            if worker.job(id).expect("job exists").status == status {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("job {id} never reached {status:?}");
    }

    #[test]
    fn counts_primes() {
        assert_eq!(count_primes(2), 1);
        assert_eq!(count_primes(100), 25);
        assert_eq!(count_primes(1_000_000), 78_498);
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_runs_to_completion() {
        let worker = worker(1, 4);

        let job = worker.submit(wait(30)).expect("accepted");
        assert_eq!(job.id, "j-1");
        assert_eq!(job.status, Status::Queued);
        until_status(&worker, "j-1", Status::Running).await;
        assert!(worker.job("j-1").unwrap().started_at.is_some());
        assert!(worker.job("j-1").unwrap().finished_at.is_none());

        tokio::time::advance(Duration::from_secs(30)).await;
        until_status(&worker, "j-1", Status::Finished).await;
        let done = worker.job("j-1").unwrap();
        assert!(done.finished_at.is_some());
        assert_eq!(done.version, VERSION);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_refuses_new_jobs_and_waits_for_running_ones() {
        let worker = worker(2, 4);
        worker.submit(wait(60)).expect("accepted");
        until_status(&worker, "j-1", Status::Running).await;
        assert!(!worker.is_draining());

        let draining = tokio::spawn({
            let worker = worker.clone();
            async move { worker.drain().await }
        });
        while !worker.is_draining() {
            tokio::task::yield_now().await;
        }

        assert_eq!(worker.submit(wait(1)), Err(SubmitError::Draining));
        assert_eq!(worker.job("j-2").map(|job| job.id), None);
        assert!(!draining.is_finished(), "drain returned with a job running");

        tokio::time::advance(Duration::from_secs(59)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert!(!draining.is_finished(), "drain returned one second early");
        assert_eq!(worker.job("j-1").unwrap().status, Status::Running);

        tokio::time::advance(Duration::from_secs(1)).await;
        draining.await.expect("drain task");
        assert_eq!(worker.job("j-1").unwrap().status, Status::Finished);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_also_finishes_jobs_that_were_still_queued() {
        let worker = worker(1, 4);
        worker.submit(wait(10)).expect("accepted");
        worker.submit(wait(10)).expect("accepted");
        until_status(&worker, "j-1", Status::Running).await;
        assert_eq!(worker.job("j-2").unwrap().status, Status::Queued);

        let draining = tokio::spawn({
            let worker = worker.clone();
            async move { worker.drain().await }
        });
        tokio::time::advance(Duration::from_secs(10)).await;
        until_status(&worker, "j-2", Status::Running).await;
        assert!(!draining.is_finished(), "drain returned with a job queued");

        tokio::time::advance(Duration::from_secs(10)).await;
        draining.await.expect("drain task");
        assert_eq!(worker.job("j-1").unwrap().status, Status::Finished);
        assert_eq!(worker.job("j-2").unwrap().status, Status::Finished);
    }

    #[tokio::test(start_paused = true)]
    async fn drain_with_nothing_running_returns_at_once() {
        let worker = worker(2, 4);

        worker.drain().await;

        assert!(worker.is_draining());
        assert_eq!(worker.submit(wait(1)), Err(SubmitError::Draining));
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_queue_refuses_jobs_until_one_starts() {
        let worker = worker(1, 2);
        worker.submit(wait(10)).expect("accepted");
        until_status(&worker, "j-1", Status::Running).await;
        worker.submit(wait(10)).expect("fits the queue");
        worker.submit(wait(10)).expect("fits the queue");

        assert_eq!(worker.submit(wait(10)), Err(SubmitError::QueueFull));
        assert_eq!(worker.job("j-4").map(|job| job.id), None);

        tokio::time::advance(Duration::from_secs(10)).await;
        until_status(&worker, "j-2", Status::Running).await;
        worker.submit(wait(10)).expect("a place opened up");
    }

    #[tokio::test(start_paused = true)]
    async fn rejects_invalid_tasks_without_using_an_id() {
        let worker = worker(1, 4);

        assert_eq!(
            worker.submit(wait(0)),
            Err(SubmitError::Invalid("seconds must be 1 to 86400".into()))
        );
        assert_eq!(
            worker.submit(Task::Primes { limit: 1 }),
            Err(SubmitError::Invalid("limit must be 2 to 10000000".into()))
        );
        assert_eq!(worker.submit(wait(1)).expect("accepted").id, "j-1");
    }

    #[tokio::test]
    async fn a_cpu_job_reports_its_result() {
        let worker = worker(1, 4);

        worker
            .submit(Task::Primes { limit: 1_000 })
            .expect("accepted");
        worker.drain().await;

        let job = worker.job("j-1").unwrap();
        assert_eq!(job.status, Status::Finished);
        assert_eq!(job.result, Some(serde_json::json!({ "primes": 168 })));
    }
}
