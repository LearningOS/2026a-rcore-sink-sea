# Lab 1：多道程序与分时多任务

## 1. 实验目标

本实验基于 rCore 第三章内核，主要目标如下：

1. 理解任务控制块、任务状态以及轮转调度的基本过程。
2. 理解用户态通过 `ecall` 进入内核、完成系统调用并返回的流程。
3. 为每个任务分别统计系统调用次数。
4. 实现 `sys_trace`，支持读写一个字节的用户内存以及查询系统调用次数。

## 2. 实验环境

- 架构：RISC-V 64
- Rust 工具链：`nightly-2024-05-02`
- 编译目标：`riscv64gc-unknown-none-elf`
- 模拟器：QEMU 8.2.2
- 测试分支：`ch3`

## 3. 原理分析

### 3.1 任务切换与调度

内核使用 `TaskControlBlock` 保存各任务的状态和任务上下文。任务可能处于 `UnInit`、`Ready`、`Running` 或 `Exited` 状态。当前任务主动调用 `yield`，或者发生时钟中断时，内核将当前任务重新标记为 `Ready`，随后从任务表中循环查找下一个 `Ready` 任务。任务退出后被标记为 `Exited`，不会再次参与调度。

真正切换任务时，`__switch` 保存当前任务的内核上下文，并恢复下一个任务的上下文。调用 `__switch` 前必须释放 `TaskManagerInner` 的可变借用，否则任务切换后可能长期持有借用，后续访问任务管理器时会发生冲突。

### 3.2 系统调用路径

用户程序把系统调用号放入 `a7`，参数放入 `a0` 至 `a2`，然后执行 `ecall`。陷阱处理函数识别 `UserEnvCall`，将 `sepc` 增加 4，并把系统调用号和参数交给统一的 `syscall` 分发函数。分发函数调用具体实现，最终将返回值写回 `a0`。

本实验要求查询动作本身也计入 `sys_trace` 次数。因此，计数必须发生在分发具体系统调用之前：

```rust
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    increment_syscall_count(syscall_id);
    match syscall_id {
        // 分发到具体系统调用
    }
}
```

## 4. 设计与实现

### 4.1 按任务统计系统调用

在 `TaskManagerInner` 中增加二维计数表：

```rust
syscall_times: [[u32; MAX_SYSCALL_NUM]; MAX_APP_NUM]
```

第一维对应任务编号，第二维对应系统调用号。计数表与任务表使用相同的任务下标，因此调度器切换 `current_task` 后，统计和查询会自然地作用于新任务。计数器使用 `u32`，既足以覆盖本实验的调用次数，也能控制静态内存占用。

`increment_syscall_count` 在系统调用入口增加当前任务的对应计数；`syscall_count` 查询当前任务的计数。两者都通过 `UPSafeCell::exclusive_access` 访问内部数据，并对系统调用号进行边界检查，避免数组越界。

### 4.2 实现 `sys_trace`

`sys_trace(trace_request, id, data)` 支持三类请求：

| `trace_request` | 功能 | 返回值 |
| --- | --- | --- |
| `0` | 从地址 `id` 读取一个字节 | 读取到的无符号字节值 |
| `1` | 向地址 `id` 写入 `data` 的低八位 | 成功时返回 `0` |
| `2` | 查询编号为 `id` 的系统调用次数 | 当前任务的调用次数 |
| 其他 | 非法请求 | `-1` |

读写操作使用 `read_volatile` 和 `write_volatile`：

```rust
match trace_request {
    0 => unsafe { (id as *const u8).read_volatile() as isize },
    1 => {
        unsafe { (id as *mut u8).write_volatile(data as u8) };
        0
    }
    2 => syscall_count(id).map_or(-1, |count| count as isize),
    _ => -1,
}
```

第三章尚未引入虚拟地址空间和跨地址空间的数据复制，所以这里直接解引用用户提供的地址。该做法满足本章测试，但不能安全处理任意地址；后续引入页表后，应先进行地址翻译和权限检查。

## 5. 测试与结果

使用课程检查器运行：

```bash
make -C ci-user test CHAPTER=3
```

检查器中的第三章运行时测试全部通过，结果为 `7/7`。通过的内容包括：

- 获取时间与休眠测试；
- 主动让出 CPU 和分时调度测试；
- `sys_trace` 的内存读写测试；
- 各任务独立的系统调用计数测试。

## 6. 测试现象分析

### 6.1 为什么第一次查询 `SYSCALL_TRACE` 得到 2

第一次执行 `count_syscall(SYSCALL_TRACE)` 时，此前没有显式查询过两次，但当前查询也属于一次 `sys_trace`。由于内核先增加计数、再执行查询，所以第一次查询返回的计数已经包含当前调用。后续每次查询其他系统调用号也会再次调用 `sys_trace`，因此 `SYSCALL_TRACE` 的计数持续增加。

### 6.2 为什么一次输出产生两次 `write`

测试调用了 `println!("string from task trace test\n")`。用户库的 `println!` 宏会自动在格式字符串末尾再添加一个换行符，因此最终存在两个连续换行。控制台缓冲区遇到每个换行都会刷新一次，而每次刷新都会调用一次 `write`，所以统计结果为两次。

## 7. 实验总结

本实验完成了 `sys_trace` 及按任务隔离的系统调用统计。实现过程中最关键的是选择正确的计数位置：只有在统一分发入口、具体处理之前计数，才能覆盖所有系统调用，并使 `sys_trace` 正确统计自身。实验也说明了任务上下文之外仍有需要随任务隔离的内核状态；系统调用统计虽然不参与上下文切换，但必须通过当前任务编号正确关联到对应任务。

