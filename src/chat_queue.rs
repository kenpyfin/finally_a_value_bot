use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::warn;

type BoxTaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

struct QueuedTask {
    fut: BoxTaskFuture,
}

struct ChatLane {
    tx: mpsc::UnboundedSender<QueuedTask>,
    /// Number of enqueued + currently running tasks for this chat lane.
    pending: usize,
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
        let mut lanes = self.lanes.lock().await;
        let lane = if let Some(existing) = lanes.get_mut(&chat_id) {
            existing
        } else {
            let (tx, mut rx) = mpsc::unbounded_channel::<QueuedTask>();
            let queue = self.clone();
            tokio::spawn(async move {
                while let Some(task) = rx.recv().await {
                    task.fut.await;
                    queue.task_finished(chat_id).await;
                }
            });
            lanes.insert(chat_id, ChatLane { tx, pending: 0 });
            lanes
                .get_mut(&chat_id)
                .expect("lane inserted for chat queue")
        };

        lane.pending = lane.pending.saturating_add(1);
        let position = lane.pending;
        if lane.tx.send(QueuedTask { fut: Box::pin(fut) }).is_err() {
            // Sender should only fail if lane worker unexpectedly exited.
            lane.pending = lane.pending.saturating_sub(1);
            warn!(chat_id, "chat queue worker unavailable; task dropped");
        }
        position
    }

    async fn task_finished(&self, chat_id: i64) {
        let mut lanes = self.lanes.lock().await;
        let remove = if let Some(lane) = lanes.get_mut(&chat_id) {
            lane.pending = lane.pending.saturating_sub(1);
            lane.pending == 0
        } else {
            false
        };
        if remove {
            lanes.remove(&chat_id);
        }
    }
}
