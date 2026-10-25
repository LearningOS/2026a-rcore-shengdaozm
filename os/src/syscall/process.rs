//! Process management syscalls
use crate::{
    task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, current_user_token, syscall_count, mmap_current, munmap_current},
    mm::{copy_from_user, copy_to_user},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug,Clone,Copy)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

// 之前都是物理地址，直接写入内存
// 现在加入虚拟内存后，用户的虚拟地址内核看不到
/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    if ts.is_null() {
        return -1;
    }
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1000000,
        usec: us % 1000000,
    };
    unsafe {
        if copy_to_user(current_user_token(), ts as usize, &time_val).is_err() {
            return -1;
        }
    }
    0
}

/*
如果 trace_request 为 0，则 id 应被视作 *const u8 ，表示读取当前任务 id 地址处一个字节的无符号整数值。此时应忽略 data 参数。返回值为 id 地址处的值。
如果 trace_request 为 1，则 id 应被视作 *mut u8 ，表示写入 data （作为 u8，即只考虑最低位的一个字节）到该用户程序 id 地址处。返回值应为0。
如果 trace_request 为 2，表示查询当前任务调用编号为 id 的系统调用的次数，返回值为这个调用次数。本次调用也计入统计 。
否则，忽略其他参数，返回值为 -1。
 */
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {
            // read one byte from the user address `id` (treated as *const u8)
            if id == 0 {
                return -1;
            }
            // 安全地从用户空间读取一个字节：校验地址已映射、可读且为用户态页，
            // 任何不满足都会返回 Err，而不会像 translated_byte_buffer 那样 unwrap 崩溃。
            let mut byte: u8 = 0;
            unsafe {
                if copy_from_user(current_user_token(), &mut byte, id).is_err() {
                    return -1;
                }
            }
            byte as isize
        }
        1 => {
            // write `data` (lowest byte) to the user address `id` (treated as *mut u8)
            if id == 0 {
                return -1;
            }
            let byte = data as u8;
            unsafe {
                if copy_to_user(current_user_token(), id, &byte).is_err() {
                    return -1;
                }
            }
            0
        }
        2 => syscall_count(id) as isize,
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if mmap_current(start, len, prot) {
        0
    } else {
        -1
    }
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if munmap_current(start, len) {
        0
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
