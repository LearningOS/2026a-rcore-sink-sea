//! Process management syscalls
use crate::mm::{translated_byte_buffer_checked, MapPermission};
use crate::task::{
    change_program_brk, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next, syscall_count, with_current_memory_set,
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
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

/// Get time with second and microsecond, supporting a cross-page [`TimeVal`].
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let time = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (&time as *const TimeVal).cast::<u8>(),
            core::mem::size_of::<TimeVal>(),
        )
    };
    let Some(buffers) =
        translated_byte_buffer_checked(current_user_token(), ts.cast::<u8>(), bytes.len(), true)
    else {
        return -1;
    };
    let mut copied = 0;
    for buffer in buffers {
        buffer.copy_from_slice(&bytes[copied..copied + buffer.len()]);
        copied += buffer.len();
    }
    0
}

/// Trace user memory accesses or syscall counts.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => translated_byte_buffer_checked(current_user_token(), id as *const u8, 1, false)
            .map_or(-1, |buffers| buffers[0][0] as isize),
        1 => translated_byte_buffer_checked(current_user_token(), id as *const u8, 1, true).map_or(
            -1,
            |mut buffers| {
                buffers[0][0] = data as u8;
                0
            },
        ),
        2 => syscall_count(id).map_or(-1, |count| count as isize),
        _ => -1,
    }
}

/// Map anonymous user memory.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if prot == 0 || prot & !0x7 != 0 {
        return -1;
    }
    let mut permission = MapPermission::empty();
    if prot & 0x1 != 0 {
        permission |= MapPermission::R;
    }
    if prot & 0x2 != 0 {
        permission |= MapPermission::W;
    }
    if prot & 0x4 != 0 {
        permission |= MapPermission::X;
    }
    with_current_memory_set(|memory_set| memory_set.mmap(start, len, permission)) as isize - 1
}

/// Unmap user memory.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    with_current_memory_set(|memory_set| memory_set.munmap(start, len)) as isize - 1
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
