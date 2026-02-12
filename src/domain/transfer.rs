use std::path::PathBuf;
use std::time::Instant;

/// 传输方向
#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Upload,
    Download,
}

/// 传输任务状态
#[derive(Debug, Clone, PartialEq)]
pub enum TransferStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

/// 单个传输任务
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TransferTask {
    pub id: usize,
    pub direction: Direction,
    pub local_path: PathBuf,
    pub remote_path: String,
    pub file_name: String,
    pub size: u64,
    pub progress: f32,
    pub status: TransferStatus,
    pub started_at: Option<Instant>,
}

/// 传输队列
pub struct TransferQueue {
    tasks: Vec<TransferTask>,
    next_id: usize,
}

impl TransferQueue {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 0,
        }
    }

    pub fn enqueue(
        &mut self,
        direction: Direction,
        local_path: PathBuf,
        remote_path: String,
        file_name: String,
        size: u64,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(TransferTask {
            id,
            direction,
            local_path,
            remote_path,
            file_name,
            size,
            progress: 0.0,
            status: TransferStatus::Pending,
            started_at: None,
        });
        id
    }

    /// 获取下一个待处理任务
    #[allow(dead_code)]
    pub fn next_pending(&mut self) -> Option<&mut TransferTask> {
        self.tasks
            .iter_mut()
            .find(|t| t.status == TransferStatus::Pending)
    }

    /// 更新任务进度
    pub fn update_progress(&mut self, id: usize, progress: f32) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
        {
            if task.started_at.is_none() {
                task.started_at = Some(Instant::now());
            }
            task.progress = progress;
            task.status = TransferStatus::InProgress;
        }
    }

    /// 标记任务完成
    pub fn mark_completed(&mut self, id: usize) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
        {
            task.progress = 1.0;
            task.status = TransferStatus::Completed;
        }
    }

    /// 标记任务失败
    pub fn mark_failed(&mut self, id: usize, error: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
        {
            task.status = TransferStatus::Failed(error);
        }
    }

    /// 获取所有任务的快照
    pub fn snapshot(&self) -> Vec<TransferTask> {
        self.tasks.clone()
    }

    /// 清除已完成的任务
    pub fn clear_completed(&mut self) {
        self.tasks
            .retain(|t| t.status != TransferStatus::Completed);
    }

    /// 重试失败的任务，重置为 Pending 状态
    pub fn retry(&mut self, id: usize) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            if matches!(task.status, TransferStatus::Failed(_)) {
                task.status = TransferStatus::Pending;
                task.progress = 0.0;
                task.started_at = None;
                return true;
            }
        }
        false
    }

    /// 根据 id 获取任务的克隆
    pub fn get_task(&self, id: usize) -> Option<TransferTask> {
        self.tasks.iter().find(|t| t.id == id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_queue_with_task() -> (TransferQueue, usize) {
        let mut q = TransferQueue::new();
        let id = q.enqueue(
            Direction::Upload,
            PathBuf::from("/local/file.txt"),
            "/remote/file.txt".to_string(),
            "file.txt".to_string(),
            1024,
        );
        (q, id)
    }

    #[test]
    fn test_enqueue_returns_incremental_ids() {
        let mut q = TransferQueue::new();
        let id0 = q.enqueue(Direction::Upload, PathBuf::from("a"), "r".into(), "a".into(), 0);
        let id1 = q.enqueue(Direction::Download, PathBuf::from("b"), "r".into(), "b".into(), 0);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
    }

    #[test]
    fn test_enqueue_sets_pending() {
        let (q, _) = make_queue_with_task();
        let snap = q.snapshot();
        assert_eq!(snap[0].status, TransferStatus::Pending);
        assert_eq!(snap[0].progress, 0.0);
    }

    #[test]
    fn test_next_pending() {
        let (mut q, id) = make_queue_with_task();
        let task = q.next_pending().unwrap();
        assert_eq!(task.id, id);
    }

    #[test]
    fn test_update_progress() {
        let (mut q, id) = make_queue_with_task();
        q.update_progress(id, 0.5);
        let snap = q.snapshot();
        assert_eq!(snap[0].progress, 0.5);
        assert_eq!(snap[0].status, TransferStatus::InProgress);
    }

    #[test]
    fn test_mark_completed() {
        let (mut q, id) = make_queue_with_task();
        q.mark_completed(id);
        let snap = q.snapshot();
        assert_eq!(snap[0].status, TransferStatus::Completed);
        assert_eq!(snap[0].progress, 1.0);
    }

    #[test]
    fn test_mark_failed() {
        let (mut q, id) = make_queue_with_task();
        q.mark_failed(id, "timeout".to_string());
        let snap = q.snapshot();
        assert_eq!(snap[0].status, TransferStatus::Failed("timeout".to_string()));
    }

    #[test]
    fn test_clear_completed() {
        let mut q = TransferQueue::new();
        let id0 = q.enqueue(Direction::Upload, PathBuf::from("a"), "r".into(), "a".into(), 0);
        let _id1 = q.enqueue(Direction::Upload, PathBuf::from("b"), "r".into(), "b".into(), 0);
        q.mark_completed(id0);
        q.clear_completed();
        let snap = q.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].status, TransferStatus::Pending);
    }
}
