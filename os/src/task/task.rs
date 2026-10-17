//! Types related to task management

use super::TaskContext;
use crate::config::MAX_SYSCALL_NUM;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// syscall records
    /// WARN: 系统调用数目不能超过MAX_SYSCALL_NUM且调用次数不能超过u32
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
}

impl TaskControlBlock {
    /// panicking on an out-of-bounds index.
    pub fn record_syscall(&mut self, syscall_id: usize) {
        self.syscall_times[syscall_id] += 1;
    }

    /// Number of times this task invoked the syscall `syscall_id`.
    /// Unknown ids simply report 0.
    pub fn syscall_count(&self, syscall_id: usize) -> u32 {
        self.syscall_times[syscall_id]
    }
}

/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
