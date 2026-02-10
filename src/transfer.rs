use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
        });
        id
    }

    /// 获取下一个待处理任务
    pub fn next_pending(&mut self) -> Option<&mut TransferTask> {
        self.tasks
            .iter_mut()
            .find(|t| t.status == TransferStatus::Pending)
    }

    /// 更新任务进度
    pub fn update_progress(&mut self, id: usize, progress: f32) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id)
        {
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
}