use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::warn;

type BoxTaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type RunCancel = Arc<AtomicBool>;
type RunRegistryValue = (i64, RunCancel);
type RunRegistry = HashMap<String, RunRegistryValue>;

/// Where the queued work originated (for web diagnostics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueSource {
    Web,
    Telegram,
    Discord,
    Whatsapp,
    Scheduler,
}

impl QueueSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueueSource::Web => "web",
            QueueSource::Telegram => "telegram",
            QueueSource::Discord => "discord",
            QueueSource::Whatsapp => "whatsapp",
            QueueSource::Scheduler => "scheduler",
        }
    }
}

struct QueuedTask {
    fut: BoxTaskFuture,
    enqueued_at: Instant,
    run_id: String,
    project_id: Option<i64>,
    workflow_id: Option<i64>,
}

struct QueueItemEntry {
    run_id: String,
    persona_id: i64,
    source: QueueSource,
    label: String,
    project_id: Option<i64>,
    workflow_id: Option<i64>,
}

struct ChatLane {
    tx: mpsc::UnboundedSender<QueuedTask>,
    /// Number of enqueued + currently running tasks for this chat lane.
    pending: usize,
    started_at: Instant,
    last_error: Option<String>,
    current_project_id: Option<i64>,
    current_workflow_id: Option<i64>,
    oldest_enqueued_at: Option<Instant>,
    /// FIFO order: front is running or next to run.
    items: VecDeque<QueueItemEntry>,
    /// Which `run_id` is currently executing (must match `items.front()` while running).
    current_run_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct QueueTaskMeta {
    pub project_id: Option<i64>,
    pub workflow_id: Option<i64>,
}

/// Metadata for one enqueue; `run_id` must be unique per enqueue (e.g. UUID).
#[derive(Clone, Debug)]
pub struct QueueEnqueueMeta {
    pub run_id: String,
    pub persona_id: i64,
    pub source: QueueSource,
    pub label: String,
    pub project_id: Option<i64>,
    pub workflow_id: Option<i64>,
}

impl QueueEnqueueMeta {
    /// For callers that only need project/workflow (legacy `QueueTaskMeta` shape).
    pub fn from_task_meta(
        run_id: String,
        persona_id: i64,
        source: QueueSource,
        label: String,
        m: QueueTaskMeta,
    ) -> Self {
        Self {
            run_id,
            persona_id,
            source,
            label,
            project_id: m.project_id,
            workflow_id: m.workflow_id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct QueueItemDiagnostic {
    pub run_id: String,
    pub persona_id: i64,
    pub source: String,
    pub label: String,
    pub state: String,
    pub project_id: Option<i64>,
    pub workflow_id: Option<i64>,
    pub position: usize,
}

#[derive(Clone, Debug)]
pub struct LaneDiagnostic {
    pub chat_id: i64,
    pub pending: usize,
    pub active_for_ms: u128,
    pub oldest_wait_ms: u128,
    pub last_error: Option<String>,
    pub project_id: Option<i64>,
    pub workflow_id: Option<i64>,
    pub items: Vec<QueueItemDiagnostic>,
}

#[derive(Clone, Default)]
pub struct ChatRunQueue {
    lanes: Arc<Mutex<HashMap<i64, ChatLane>>>,
    /// `run_id` -> (`chat_id`, cancel flag) for `request_cancel`.
    runs: Arc<Mutex<RunRegistry>>,
}

impl ChatRunQueue {
    /// Enqueue a task for a chat-scoped FIFO lane.
    /// Returns 1-based queue position and the cancel handle for cooperative cancellation.
    pub async fn enqueue<F, Fut>(&self, chat_id: i64, make_fut: F) -> (usize, Arc<AtomicBool>)
    where
        F: FnOnce(Arc<AtomicBool>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let run_id = uuid::Uuid::new_v4().to_string();
        let meta = QueueEnqueueMeta::from_task_meta(
            run_id,
            0,
            QueueSource::Web,
            String::new(),
            QueueTaskMeta::default(),
        );
        self.enqueue_with_meta(chat_id, meta, make_fut).await
    }

    pub async fn enqueue_with_meta<F, Fut>(
        &self,
        chat_id: i64,
        meta: QueueEnqueueMeta,
        make_fut: F,
    ) -> (usize, Arc<AtomicBool>)
    where
        F: FnOnce(Arc<AtomicBool>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let run_id = meta.run_id.clone();
        {
            let mut guard = self.runs.lock().await;
            guard.insert(run_id.clone(), (chat_id, cancel.clone()));
        }

        let entry = QueueItemEntry {
            run_id: run_id.clone(),
            persona_id: meta.persona_id,
            source: meta.source.clone(),
            label: meta.label.clone(),
            project_id: meta.project_id,
            workflow_id: meta.workflow_id,
        };

        let fut = make_fut(cancel.clone());

        let mut lanes = self.lanes.lock().await;
        let lane = if let Some(existing) = lanes.get_mut(&chat_id) {
            existing
        } else {
            let queue = self.clone();
            let (tx, mut rx) = mpsc::unbounded_channel::<QueuedTask>();
            let chat_id_worker = chat_id;
            tokio::spawn(async move {
                while let Some(task) = rx.recv().await {
                    let started_wait = task.enqueued_at.elapsed();
                    if started_wait.as_secs() >= 300 {
                        warn!(
                            chat_id = chat_id_worker,
                            wait_ms = started_wait.as_millis(),
                            project_id = task.project_id,
                            workflow_id = task.workflow_id,
                            "queued task waited a long time before starting"
                        );
                    }
                    let run_id = task.run_id.clone();
                    {
                        let mut lanes = queue.lanes.lock().await;
                        if let Some(lane) = lanes.get_mut(&chat_id_worker) {
                            lane.current_run_id = Some(run_id.clone());
                        }
                    }
                    let skip = {
                        let guard = queue.runs.lock().await;
                        guard
                            .get(&run_id)
                            .map(|(_, c)| c.load(Ordering::SeqCst))
                            .unwrap_or(false)
                    };
                    if skip {
                        queue.finish_one(chat_id_worker, &run_id).await;
                        continue;
                    }
                    task.fut.await;
                    queue.finish_one(chat_id_worker, &run_id).await;
                }
            });
            lanes.insert(
                chat_id,
                ChatLane {
                    tx,
                    pending: 0,
                    started_at: Instant::now(),
                    last_error: None,
                    current_project_id: None,
                    current_workflow_id: None,
                    oldest_enqueued_at: None,
                    items: VecDeque::new(),
                    current_run_id: None,
                },
            );
            lanes
                .get_mut(&chat_id)
                .expect("lane inserted for chat queue")
        };

        lane.pending = lane.pending.saturating_add(1);
        lane.current_project_id = meta.project_id.or(lane.current_project_id);
        lane.current_workflow_id = meta.workflow_id.or(lane.current_workflow_id);
        let now = Instant::now();
        if lane.oldest_enqueued_at.map_or(true, |t| now < t) {
            lane.oldest_enqueued_at = Some(now);
        }
        lane.items.push_back(entry);
        let position = lane.pending;

        if lane
            .tx
            .send(QueuedTask {
                fut: Box::pin(fut),
                enqueued_at: now,
                run_id: run_id.clone(),
                project_id: meta.project_id,
                workflow_id: meta.workflow_id,
            })
            .is_err()
        {
            lane.pending = lane.pending.saturating_sub(1);
            lane.items.pop_back();
            lane.last_error = Some("lane worker unavailable".to_string());
            warn!(chat_id, "chat queue worker unavailable; task dropped");
            let mut guard = self.runs.lock().await;
            guard.remove(&run_id);
            return (0, cancel);
        }
        (position, cancel)
    }

    async fn finish_one(&self, chat_id: i64, run_id: &str) {
        {
            let mut guard = self.runs.lock().await;
            guard.remove(run_id);
        }
        let mut lanes = self.lanes.lock().await;
        let remove_lane = if let Some(lane) = lanes.get_mut(&chat_id) {
            if lane.current_run_id.as_deref() == Some(run_id) {
                lane.current_run_id = None;
            }
            if let Some(front) = lane.items.front() {
                if front.run_id == run_id {
                    lane.items.pop_front();
                }
            }
            lane.pending = lane.pending.saturating_sub(1);
            if lane.pending == 0 {
                lane.oldest_enqueued_at = None;
            }
            lane.pending == 0
        } else {
            false
        };
        if remove_lane {
            lanes.remove(&chat_id);
        }
    }

    /// Request cooperative cancellation for `run_id`. Returns `true` if the run was known and `chat_id` matches.
    pub async fn request_cancel(&self, run_id: &str, chat_id: i64) -> bool {
        let cancel = {
            let guard = self.runs.lock().await;
            guard.get(run_id).and_then(|(cid, c)| {
                if *cid == chat_id {
                    Some(c.clone())
                } else {
                    None
                }
            })
        };
        if let Some(c) = cancel {
            c.store(true, Ordering::SeqCst);
            return true;
        }
        false
    }

    pub async fn diagnostics(&self) -> Vec<LaneDiagnostic> {
        let lanes = self.lanes.lock().await;
        let now = Instant::now();
        lanes
            .iter()
            .map(|(chat_id, lane)| {
                let items: Vec<QueueItemDiagnostic> = lane
                    .items
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        let state = if Some(e.run_id.as_str()) == lane.current_run_id.as_deref() {
                            "running"
                        } else {
                            "queued"
                        };
                        QueueItemDiagnostic {
                            run_id: e.run_id.clone(),
                            persona_id: e.persona_id,
                            source: e.source.as_str().to_string(),
                            label: e.label.clone(),
                            state: state.to_string(),
                            project_id: e.project_id,
                            workflow_id: e.workflow_id,
                            position: i + 1,
                        }
                    })
                    .collect();
                LaneDiagnostic {
                    chat_id: *chat_id,
                    pending: lane.pending,
                    active_for_ms: now.duration_since(lane.started_at).as_millis(),
                    oldest_wait_ms: lane
                        .oldest_enqueued_at
                        .map(|t| now.duration_since(t).as_millis())
                        .unwrap_or(0),
                    last_error: lane.last_error.clone(),
                    project_id: lane.current_project_id,
                    workflow_id: lane.current_workflow_id,
                    items,
                }
            })
            .collect()
    }
}
