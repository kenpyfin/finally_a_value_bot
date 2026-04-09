use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::warn;

type BoxTaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct QueuedTask {
    fut: BoxTaskFuture,
    enqueued_at: Instant,
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
}

#[derive(Clone, Debug, Default)]
pub struct QueueTaskMeta {
    pub project_id: Option<i64>,
    pub workflow_id: Option<i64>,
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
}

#[derive(Clone, Default)]
pub struct ChatRunQueue {
    lanes: Arc<Mutex<HashMap<i64, ChatLane>>>,
}

impl ChatRunQueue {
    /// Enqueue a task for a chat-scoped FIFO lane.
    /// Returns 1-based queue position (1 means no wait ahead).
    pub async fn enqueue<F>(&self, chat_id: i64, fut: F) -> usize
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.enqueue_with_meta(chat_id, QueueTaskMeta::default(), fut).await
    }

    pub async fn enqueue_with_meta<F>(&self, chat_id: i64, meta: QueueTaskMeta, fut: F) -> usize
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut lanes = self.lanes.lock().await;
        let lane = if let Some(existing) = lanes.get_mut(&chat_id) {
            existing
        } else {
            let (tx, mut rx) = mpsc::unbounded_channel::<QueuedTask>();
            let queue = self.clone();
            tokio::spawn(async move {
                while let Some(task) = rx.recv().await {
                    let started_wait = task.enqueued_at.elapsed();
                    if started_wait.as_secs() >= 300 {
                        warn!(
                            chat_id,
                            wait_ms = started_wait.as_millis(),
                            project_id = task.project_id,
                            workflow_id = task.workflow_id,
                            "queued task waited a long time before starting"
                        );
                    }
                    task.fut.await;
                    queue.task_finished(chat_id).await;
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
        let position = lane.pending;
        if lane
            .tx
            .send(QueuedTask {
                fut: Box::pin(fut),
                enqueued_at: now,
                project_id: meta.project_id,
                workflow_id: meta.workflow_id,
            })
            .is_err()
        {
            // Sender should only fail if lane worker unexpectedly exited.
            lane.pending = lane.pending.saturating_sub(1);
            lane.last_error = Some("lane worker unavailable".to_string());
            warn!(chat_id, "chat queue worker unavailable; task dropped");
        }
        position
    }

    async fn task_finished(&self, chat_id: i64) {
        let mut lanes = self.lanes.lock().await;
        let remove = if let Some(lane) = lanes.get_mut(&chat_id) {
            lane.pending = lane.pending.saturating_sub(1);
            if lane.pending == 0 {
                lane.oldest_enqueued_at = None;
            }
            lane.pending == 0
        } else {
            false
        };
        if remove {
            lanes.remove(&chat_id);
        }
    }

    pub async fn diagnostics(&self) -> Vec<LaneDiagnostic> {
        let lanes = self.lanes.lock().await;
        let now = Instant::now();
        lanes
            .iter()
            .map(|(chat_id, lane)| LaneDiagnostic {
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
            })
            .collect()
    }
}
