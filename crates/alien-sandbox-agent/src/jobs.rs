//! Detached command jobs: run past one request, polled for output, cancelled by killing the group.
//!
//! One `POST /` call cannot answer for a command that runs longer than the execute proxy holds a
//! single call open (~30s), whatever deadline it was given. A job runs the command detached under
//! the agent's own deadline, buffers its frames, and returns them across as many short polls as the
//! command takes. Nothing here bounds the command — [`exec::stream`] and its deadline still do; a
//! job only decouples the command's lifetime from a single request's.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::error::{ErrorData, Result};
use crate::exec::{self, ExecIdentity, ExecRequest, Frame, FRAME_CHANNEL_DEPTH};
use alien_error::AlienError;

/// The most jobs one session holds at once.
///
/// A supervisor of untrusted code cannot retain job output without a ceiling: output is kept for
/// replay until a later start evicts it or the session ends, and unbounded retention is a
/// memory-exhaustion path. The worst case is `MAX_JOBS` × two streams × `output_cap` of buffered
/// frames — ≈128 MiB at the default 4 MiB cap — and a start is refused once every slot holds a
/// still-running job. This constant is the knob if that ceiling is too high for a given image.
const MAX_JOBS: usize = 16;

/// How a job ended, lifted out of its terminal frame so a poll reports it in the response envelope
/// rather than as a frame the caller has to find and interpret.
#[derive(Debug, Clone)]
pub enum JobOutcome {
    /// The command exited on its own.
    Exited {
        /// Process exit code
        code: i32,
        /// Set when output was cut short by `output_cap` rather than by the command ending
        truncated: bool,
    },
    /// The command did not exit normally — a deadline, a failed spawn, or a cancellation.
    Failed {
        /// Machine-readable cause, e.g. `deadlineExceeded`
        code: String,
        /// Human-readable detail
        message: String,
    },
}

/// What a poll sees of a job: the output it asked for and, once the job has ended, how it did.
pub struct JobSnapshot {
    /// Output frames after the polled sequence; `Stdout`/`Stderr` only.
    pub frames: Vec<Frame>,
    /// `None` while the job is still running.
    pub outcome: Option<JobOutcome>,
}

/// One job's buffered state, shared between its collector task and every poll.
struct Buffer {
    /// Output frames in production order; the terminal frame is captured in `outcome`, not here.
    frames: Vec<Frame>,
    /// `None` until the terminal frame arrives or the job is cancelled.
    outcome: Option<JobOutcome>,
    /// Set once a poll has returned the terminal outcome, so the cap never evicts a result a
    /// caller has not yet read.
    terminal_delivered: bool,
}

struct Job {
    buffer: Mutex<Buffer>,
    /// Taken by the first cancel. Dropping the collector's receiver is what kills the group, so the
    /// signal only has to reach the collector once.
    cancel: Mutex<Option<oneshot::Sender<()>>>,
    /// Start order, so the oldest evictable job is the one reclaimed under the cap.
    ordinal: u64,
}

/// The jobs one session is running or retaining, behind interior mutability so the shared
/// [`AgentState`](crate::server::AgentState) it lives in stays immutable.
pub struct JobRegistry {
    jobs: Mutex<HashMap<String, Arc<Job>>>,
    ordinal: AtomicU64,
    capacity: usize,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::with_capacity(MAX_JOBS)
    }

    fn with_capacity(capacity: usize) -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            ordinal: AtomicU64::new(0),
            capacity,
        }
    }

    /// Starts a detached job and returns its id.
    ///
    /// The request is validated and a slot reserved before anything spawns, so an invalid command
    /// or a full registry is refused as an error rather than as a job that instantly fails.
    pub fn start(
        &self,
        request: ExecRequest,
        working_directory: PathBuf,
        identity: ExecIdentity,
        output_cap: usize,
    ) -> Result<String> {
        request.validate()?;

        let id = Uuid::new_v4().to_string();
        let job = Arc::new(Job {
            buffer: Mutex::new(Buffer {
                frames: Vec::new(),
                outcome: None,
                terminal_delivered: false,
            }),
            cancel: Mutex::new(None),
            ordinal: self.ordinal.fetch_add(1, Ordering::Relaxed),
        });

        {
            let mut jobs = self.lock();
            self.make_room(&mut jobs)?;
            jobs.insert(id.clone(), Arc::clone(&job));
        }

        let (frames_tx, frames_rx) = mpsc::channel(FRAME_CHANNEL_DEPTH);
        let (cancel_tx, cancel_rx) = oneshot::channel();
        *job.cancel.lock().expect("no panic holds a job lock") = Some(cancel_tx);

        tokio::spawn(async move {
            exec::stream(
                &request,
                Some(&working_directory),
                identity,
                output_cap,
                frames_tx,
            )
            .await;
        });
        tokio::spawn(collect(frames_rx, cancel_rx, job));

        Ok(id)
    }

    /// Returns a job's buffered output strictly after `since_seq`, or `None` when no such job.
    ///
    /// `since_seq` is exclusive so a poll retried after a lost response returns the frames the
    /// caller is still missing rather than duplicating ones it already has. `None` returns from the
    /// first frame — the value a caller passes before it has received any.
    ///
    /// Sequence numbers may skip: a line dropped at `output_cap` still consumes one, so a gap is
    /// output that was truncated, not a frame lost in transit. The terminal `truncated` flag is
    /// what reports it; a caller must not treat a gap as a frame still to come.
    pub fn poll(&self, id: &str, since_seq: Option<u64>) -> Option<JobSnapshot> {
        let job = Arc::clone(self.lock().get(id)?);
        let mut buffer = job.buffer.lock().expect("no panic holds a job lock");
        let frames = buffer
            .frames
            .iter()
            .filter(|frame| match frame_seq(frame) {
                Some(seq) => since_seq.is_none_or(|since| seq > since),
                None => false,
            })
            .cloned()
            .collect();
        let outcome = buffer.outcome.clone();
        // The caller has now seen the terminal result, so the cap may reclaim this slot.
        if outcome.is_some() {
            buffer.terminal_delivered = true;
        }
        Some(JobSnapshot { frames, outcome })
    }

    /// Signals a job to cancel, killing its process group. Returns whether the job existed.
    pub fn cancel(&self, id: &str) -> bool {
        let Some(job) = self.lock().get(id).map(Arc::clone) else {
            return false;
        };
        if let Some(signal) = job.cancel.lock().expect("no panic holds a job lock").take() {
            let _ = signal.send(());
        }
        true
    }

    /// How many jobs the session is holding, running and retained alike. Never above the capacity.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether a job has reached a terminal outcome, read without a poll so a test can await
    /// completion without marking the result delivered.
    #[cfg(test)]
    fn is_finished(&self, id: &str) -> bool {
        self.lock().get(id).is_some_and(|job| {
            job.buffer
                .lock()
                .expect("no panic holds a job lock")
                .outcome
                .is_some()
        })
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Arc<Job>>> {
        self.jobs.lock().expect("no panic holds the registry lock")
    }

    /// Frees a slot when the registry is full, evicting the oldest finished job.
    ///
    /// A running job is never evicted — its output is still being produced and its process is still
    /// alive. When every slot holds one, the start is refused rather than dropping live output.
    fn make_room(&self, jobs: &mut HashMap<String, Arc<Job>>) -> Result<()> {
        if jobs.len() < self.capacity {
            return Ok(());
        }

        // Only a finished job whose terminal result a caller has already read: evicting an
        // unread result would turn the next poll into `JobNotFound` and lose the real exit code.
        let oldest_evictable = jobs
            .iter()
            .filter(|(_, job)| {
                let buffer = job.buffer.lock().expect("no panic holds a job lock");
                buffer.outcome.is_some() && buffer.terminal_delivered
            })
            .min_by_key(|(_, job)| job.ordinal)
            .map(|(id, _)| id.clone());

        match oldest_evictable {
            Some(id) => {
                jobs.remove(&id);
                Ok(())
            }
            None => Err(AlienError::new(ErrorData::JobLimitReached {
                limit: self.capacity,
            })),
        }
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Drains a job's frames into its buffer until the command ends or a cancel arrives.
///
/// On cancel the receiver is dropped by returning, which closes [`exec::stream`]'s frame channel;
/// the shared process path turns that into `SIGKILL` on the command's process group, so a job
/// cancels the whole tree it spawned rather than only its direct child.
async fn collect(
    mut frames: mpsc::Receiver<Frame>,
    mut cancel: oneshot::Receiver<()>,
    job: Arc<Job>,
) {
    loop {
        tokio::select! {
            frame = frames.recv() => match frame {
                Some(Frame::Exit { code, truncated }) => {
                    finish(&job, JobOutcome::Exited { code, truncated });
                    return;
                }
                Some(Frame::Error { code, message }) => {
                    finish(&job, JobOutcome::Failed { code, message });
                    return;
                }
                Some(output) => {
                    job.buffer
                        .lock()
                        .expect("no panic holds a job lock")
                        .frames
                        .push(output);
                }
                // The stream always ends with a terminal frame; reaching here means the producing
                // task was dropped before it sent one, which is still a job no longer running.
                None => {
                    finish(&job, JobOutcome::Failed {
                        code: "streamEnded".to_string(),
                        message: "the command's output ended without a terminal frame".to_string(),
                    });
                    return;
                }
            },
            _ = &mut cancel => {
                finish(&job, JobOutcome::Failed {
                    code: "cancelled".to_string(),
                    message: "the job was cancelled".to_string(),
                });
                return;
            }
        }
    }
}

/// Records a job's outcome, unless one is already set.
///
/// A cancel that races the command's own terminal frame must not overwrite the real ending: the
/// first outcome to land is the one that happened.
fn finish(job: &Job, outcome: JobOutcome) {
    let mut buffer = job.buffer.lock().expect("no panic holds a job lock");
    if buffer.outcome.is_none() {
        buffer.outcome = Some(outcome);
    }
}

fn frame_seq(frame: &Frame) -> Option<u64> {
    match frame {
        Frame::Stdout { seq, .. } | Frame::Stderr { seq, .. } => Some(*seq),
        Frame::Exit { .. } | Frame::Error { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    /// The uid the test process already has. Setting a uid to its own is permitted unprivileged, so
    /// this exercises the real spawn path without needing root.
    fn same_identity() -> ExecIdentity {
        #[cfg(unix)]
        unsafe {
            ExecIdentity {
                uid: libc::getuid(),
                gid: libc::getgid(),
            }
        }
        #[cfg(not(unix))]
        ExecIdentity { uid: 0, gid: 0 }
    }

    fn request(command: &[&str], deadline_ms: u64) -> ExecRequest {
        ExecRequest {
            command: command.iter().map(|s| s.to_string()).collect(),
            deadline_ms,
            working_directory: None,
            env: BTreeMap::new(),
        }
    }

    fn start(registry: &JobRegistry, command: &[&str], deadline_ms: u64) -> String {
        registry
            .start(
                request(command, deadline_ms),
                std::env::temp_dir(),
                same_identity(),
                1 << 20,
            )
            .expect("a valid job starts")
    }

    /// Polls until the job is no longer running, returning its terminal snapshot. Bounded so a job
    /// that never ends fails the test rather than hanging it.
    async fn wait_for_completion(registry: &JobRegistry, id: &str) -> JobSnapshot {
        // Generous enough to outlast the longest job any test starts, including the ignored one
        // that sleeps past the execute proxy's cap; a job that never ends still fails rather than
        // hanging the run.
        for _ in 0..2400 {
            let snapshot = registry.poll(id, None).expect("the job exists");
            if snapshot.outcome.is_some() {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the job never reached a terminal state");
    }

    fn seqs(frames: &[Frame]) -> Vec<u64> {
        frames.iter().filter_map(frame_seq).collect()
    }

    fn stdout_text(frames: &[Frame]) -> String {
        let mut collected = Vec::new();
        for frame in frames {
            if let Frame::Stdout { data, .. } = frame {
                collected.extend_from_slice(&STANDARD.decode(data).expect("valid base64"));
            }
        }
        String::from_utf8(collected).expect("utf8 output")
    }

    /// The property the whole module exists for: a job runs past the call that started it, and its
    /// output is readable incrementally while it runs and in full once it ends.
    #[tokio::test]
    async fn a_job_outlives_its_start_and_streams_across_polls() {
        let registry = JobRegistry::new();

        let id = start(
            &registry,
            &["/bin/sh", "-c", "echo a; sleep 1; echo b; sleep 1; echo c"],
            30_000,
        );

        // The start returned while the command is still sleeping between its writes.
        let early = registry.poll(&id, None).expect("the job exists");
        assert!(
            early.outcome.is_none(),
            "the job must still be running right after it started: {:?}",
            stdout_text(&early.frames)
        );

        let done = wait_for_completion(&registry, &id).await;
        assert!(
            matches!(done.outcome, Some(JobOutcome::Exited { code: 0, .. })),
            "the job must exit cleanly"
        );
        assert_eq!(
            stdout_text(&done.frames)
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"],
            "the full output must survive across polls"
        );
    }

    /// A command longer than the execute proxy's single-call cap still completes and returns every
    /// line. Ignored because it spends its wall-clock; run with `--ignored`.
    #[tokio::test]
    #[ignore = "spends ~35s of wall-clock proving the cap is cleared"]
    async fn a_job_longer_than_the_proxy_cap_completes_in_full() {
        let registry = JobRegistry::new();

        let id = start(
            &registry,
            &["/bin/sh", "-c", "echo start; sleep 35; echo end"],
            60_000,
        );

        let done = wait_for_completion(&registry, &id).await;
        assert!(
            matches!(done.outcome, Some(JobOutcome::Exited { code: 0, .. })),
            "a 35s job must exit cleanly, not hit a cap: {:?}",
            done.outcome
        );
        assert_eq!(
            stdout_text(&done.frames)
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["start", "end"],
            "both the pre- and post-sleep output must arrive"
        );
    }

    /// A client that loses a response re-polls from a cursor it has already passed. Across several
    /// overlapping windows — including one that rewinds behind the last — a window must begin at
    /// exactly the frame after its cursor and never re-deliver one at or before it, so stitching
    /// the deltas rebuilds the stream with no seq repeated and none skipped. The strictly-after
    /// test polls a finished job at two fixed offsets; this walks a moving, overlapping cursor as
    /// `run_detached` does.
    #[tokio::test]
    async fn overlapping_polls_reconstruct_the_stream_exactly_once() {
        let registry = JobRegistry::new();
        let id = start(
            &registry,
            &["/bin/sh", "-c", "echo a; echo b; echo c; echo d; echo e"],
            10_000,
        );
        assert_eq!(
            seqs(&wait_for_completion(&registry, &id).await.frames),
            vec![0, 1, 2, 3, 4],
            "five output lines, seq 0..=4"
        );

        let mut covered = BTreeSet::new();
        for since in [None, Some(1), Some(0), Some(3), Some(2)] {
            let delta = seqs(&registry.poll(&id, since).expect("the job exists").frames);
            if let Some(since) = since {
                assert!(
                    delta.iter().all(|seq| *seq > since),
                    "no duplication: a window past {since} re-delivered {delta:?}"
                );
                if let Some(&first) = delta.first() {
                    assert_eq!(
                        first,
                        since + 1,
                        "no gap: a window must begin at the frame right after its cursor"
                    );
                }
            }
            covered.extend(delta);
        }
        assert_eq!(
            covered.into_iter().collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4],
            "the overlapping windows together cover every frame exactly once"
        );
    }

    /// A stale `sinceSeq` returns exactly the frames after it — no duplication of what the caller
    /// already had, no gap before what it is missing — and the same poll repeated returns the same
    /// frames, which is what makes a retried poll safe.
    #[tokio::test]
    async fn poll_returns_frames_strictly_after_since_seq() {
        let registry = JobRegistry::new();
        let id = start(
            &registry,
            &["/bin/sh", "-c", "echo a; echo b; echo c; echo d"],
            10_000,
        );

        let all = wait_for_completion(&registry, &id).await;
        assert_eq!(
            seqs(&all.frames),
            vec![0, 1, 2, 3],
            "four output lines, seq 0..=3"
        );

        let after_one = registry.poll(&id, Some(1)).expect("the job exists");
        assert_eq!(
            seqs(&after_one.frames),
            vec![2, 3],
            "strictly after 1: no dup of 0 or 1, no gap before 2"
        );

        let retried = registry.poll(&id, Some(1)).expect("the job exists");
        assert_eq!(
            seqs(&retried.frames),
            vec![2, 3],
            "a retried poll returns the same frames, never fewer or more"
        );

        let after_last = registry.poll(&id, Some(3)).expect("the job exists");
        assert!(
            after_last.frames.is_empty(),
            "nothing follows the last frame"
        );
    }

    /// Cancel kills the process group, so a process the command forked does not outlive it. Proven
    /// by a grandchild that keeps writing a marker file: after the cancel the file stops growing.
    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_kills_the_forked_child_too() {
        let registry = JobRegistry::new();
        let marker = std::env::temp_dir().join(format!("alien-job-cancel-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);

        // The grandchild is backgrounded and outlives the shell's own foreground sleep. stdout is
        // closed so it cannot hold the frame pipe open — this is about the process, not the stream.
        let script = format!(
            "(while true; do echo x >> {} ; sleep 0.05; done) >/dev/null 2>&1 &\nsleep 30",
            marker.display()
        );
        let id = start(&registry, &["/bin/sh", "-c", &script], 60_000);

        tokio::time::sleep(Duration::from_millis(500)).await;
        let before = std::fs::metadata(&marker).map(|m| m.len());

        assert!(
            registry.cancel(&id),
            "cancelling a live job reports it existed"
        );

        // Give the kill time to land, then confirm the marker stops growing.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let after_cancel = std::fs::metadata(&marker).map(|m| m.len());
        tokio::time::sleep(Duration::from_millis(500)).await;
        let later = std::fs::metadata(&marker).map(|m| m.len());
        let _ = std::fs::remove_file(&marker);

        let before = before.expect("the grandchild must have written before the cancel");
        let after_cancel = after_cancel.expect("the marker must still exist");
        let later = later.expect("the marker must still exist");
        assert!(
            before > 0,
            "the grandchild wrote nothing, so this test proves nothing"
        );
        assert_eq!(
            after_cancel, later,
            "a process the command forked outlived the cancel and is still writing"
        );

        let snapshot = registry.poll(&id, None).expect("the job exists");
        assert!(
            matches!(snapshot.outcome, Some(JobOutcome::Failed { .. })),
            "a cancelled job is done, not running"
        );
    }

    /// A finished job whose result has been read is evicted to make room once the cap is reached,
    /// so retention is bounded rather than growing with every job a session ever ran.
    #[tokio::test]
    async fn a_full_registry_evicts_the_oldest_finished_job() {
        let registry = JobRegistry::with_capacity(2);

        let first = start(&registry, &["/bin/echo", "one"], 10_000);
        let second = start(&registry, &["/bin/echo", "two"], 10_000);
        wait_for_completion(&registry, &first).await;
        wait_for_completion(&registry, &second).await;

        // The third start is at the cap, so the oldest finished job is evicted for it.
        let third = start(&registry, &["/bin/echo", "three"], 10_000);
        wait_for_completion(&registry, &third).await;

        assert_eq!(registry.len(), 2, "retention never exceeds the capacity");
        assert!(
            registry.poll(&first, None).is_none(),
            "the oldest finished job must have been evicted"
        );
        assert!(
            registry.poll(&second, None).is_some(),
            "a newer finished job is retained"
        );
        assert!(
            registry.poll(&third, None).is_some(),
            "the job that forced the eviction is retained"
        );
    }

    /// A finished job no poll has read is never evicted: dropping it would turn the caller's next
    /// poll into a not-found and lose the real exit code, so the cap refuses a new start instead.
    #[tokio::test]
    async fn an_unread_finished_job_is_not_evicted() {
        let registry = JobRegistry::with_capacity(1);

        let first = start(&registry, &["/bin/echo", "one"], 10_000);
        for _ in 0..2400 {
            if registry.is_finished(&first) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            registry.is_finished(&first),
            "the job reaches a terminal state"
        );

        // At the cap with the only result still unread, a new start is refused, not evicted.
        let refused = registry.start(
            request(&["/bin/echo", "two"], 10_000),
            std::env::temp_dir(),
            same_identity(),
            1 << 20,
        );
        assert_eq!(
            refused
                .expect_err("an unread finished result must not be evicted")
                .code,
            "JOB_LIMIT_REACHED"
        );

        // Reading it makes the slot reclaimable, so the next start then succeeds.
        assert!(registry.poll(&first, None).unwrap().outcome.is_some());
        start(&registry, &["/bin/echo", "three"], 10_000);
        assert_eq!(
            registry.len(),
            1,
            "the read result was reclaimed for the new job"
        );
    }

    /// When every slot holds a still-running job, a new start is refused rather than killing live
    /// output to make room.
    #[tokio::test]
    async fn a_registry_full_of_running_jobs_refuses_a_new_one() {
        let registry = JobRegistry::with_capacity(2);

        let first = start(&registry, &["/bin/sleep", "30"], 60_000);
        let second = start(&registry, &["/bin/sleep", "30"], 60_000);

        let refused = registry.start(
            request(&["/bin/echo", "blocked"], 10_000),
            std::env::temp_dir(),
            same_identity(),
            1 << 20,
        );
        let error = refused.expect_err("a registry full of running jobs must refuse a new job");
        assert_eq!(error.code, "JOB_LIMIT_REACHED");

        assert!(registry.cancel(&first), "the running jobs still exist");
        assert!(registry.cancel(&second));
    }

    /// An empty command is refused before a slot is reserved, so a rejected request leaves no job
    /// behind to be polled or to occupy the cap.
    #[tokio::test]
    async fn an_invalid_request_is_refused_without_reserving_a_slot() {
        let registry = JobRegistry::new();

        let refused = registry.start(
            request(&[], 10_000),
            std::env::temp_dir(),
            same_identity(),
            1 << 20,
        );
        assert_eq!(
            refused.expect_err("an empty command is invalid").code,
            "REQUEST_INVALID"
        );

        // A fresh registry with a rejected start holds nothing.
        let running = start(&registry, &["/bin/sleep", "30"], 60_000);
        assert!(registry.cancel(&running));
    }

    /// Output past the cap is dropped and the job flagged `truncated`; the sequence gap that leaves
    /// is not a frame still to come, so a poll from the last kept frame returns nothing rather than
    /// waiting on the numbers truncation consumed.
    #[tokio::test]
    async fn a_truncated_job_reports_it_and_leaves_no_pending_frame() {
        let registry = JobRegistry::new();
        let id = registry
            .start(
                request(
                    &[
                        "/bin/sh",
                        "-c",
                        "for i in 1 2 3 4 5 6 7 8; do echo aaaaaaaaaa; done",
                    ],
                    10_000,
                ),
                std::env::temp_dir(),
                same_identity(),
                25,
            )
            .expect("a valid job starts");

        let done = wait_for_completion(&registry, &id).await;
        assert!(
            matches!(
                done.outcome,
                Some(JobOutcome::Exited {
                    truncated: true,
                    ..
                })
            ),
            "output past the cap must be flagged truncated: {:?}",
            done.outcome
        );

        let last = *seqs(&done.frames)
            .last()
            .expect("some output is kept below the cap");
        assert!(
            registry
                .poll(&id, Some(last))
                .expect("the job exists")
                .frames
                .is_empty(),
            "nothing follows the last kept frame, whatever numbers truncation skipped"
        );
    }

    /// A poll or cancel for an id the session never held reports it is gone rather than inventing a
    /// running job with no output.
    #[tokio::test]
    async fn a_missing_job_is_absent_to_poll_and_cancel() {
        let registry = JobRegistry::new();
        assert!(registry.poll("nonexistent", None).is_none());
        assert!(!registry.cancel("nonexistent"));
    }
}
