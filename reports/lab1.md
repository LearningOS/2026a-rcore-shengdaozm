# 补充trace系统调用
1. 为0时，直接将id当成u8的ptr，直接访问返回一个isize即可
2. 为1时，将id当成mut u8的ptr，直接赋值即可
3. 为2时，需要统计不同的syscall的调用次数，只需要在TCB中维护一个大数组，记录每个的调用即可，由于没有哈希表，直接开数组了，然后在syscall函数处，根据syscall的id，直接将对应的数组元素加1即可

# 简答作业
1. 正确进入 U 态后，程序的特征还应有：使用 S 态特权指令，访问 S 态寄存器后会报错。 请同学们可以自行测试这些内容（运行 三个 bad 测例 (ch2b_bad_*.rs) ）， 描述程序出错行为，同时注意注明你使用的 sbi 及其版本。

**回答：**
SBI及其版本：
- **SBI：RustSBI-QEMU**，底层 `rustsbi` 库版本 **0.3.0-alpha.2**，适配 **RISC-V SBI v1.0.0**。
三个 bad 测例的出错行为（内核侧处理逻辑见 `os/src/trap/mod.rs:59-66`）：

| 测例 | 用户程序做了什么 | 触发的 trap | 内核输出 | 结果 |
| --- | --- | --- | --- | --- |
| `ch2b_bad_address.rs` | 向非法/越界的地址写数据 | `Exception::StoreFault`  access fault 而非 page fault；`StorePageFault` 与之共用同一分支） | `[kernel] PageFault in application, bad addr = 0x…, bad instruction = 0x…, kernel killed it.` | 该应用被 `exit_current_and_run_next()` 终止 |
| `ch2b_bad_instructions.rs` | 在 U 态执行 S 态特权指令（如 `sret`） | `Exception::IllegalInstruction` | `[kernel] IllegalInstruction in application, kernel killed it.` | 该应用被终止 |
| `ch2b_bad_register.rs` | 在 U 态访问 S 态 CSR（如 `sstatus`） | `Exception::IllegalInstruction` | `[kernel] IllegalInstruction in application, kernel killed it.` | 该应用被终止 |

由此验证的结论：正确进入 U 态后，用户程序**不再拥有**执行特权指令、访问特权 CSR、访问任意地址的权限，任何此类尝试都会被硬件拦截并交给 S 态内核处置；内核能够区分"应用自己出错"和"内核出错"，从而只杀掉出错的那一个应用。

2. 深入理解 trap.S 中两个函数 __alltraps 和 __restore 的作用，并回答如下问题:

1. L40：刚进入 __restore 时，sp 代表了什么值。请指出 __restore 的两种使用情景。

**回答：**

`sp` 指向当前任务内核栈上TrapContext的起始地址，`__restore` 的两种使用情景：

1. trap 处理完毕后返回用户态
2. 首次进入用户态 / 任务切换后进入用户态
两种情景下进入 `__restore` 时 `sp` 的含义完全一致——都指向内核栈上的 TrapContext

2. L43-L48：这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释。
```c++
ld t0, 32*8(sp)
ld t1, 33*8(sp)
ld t2, 2*8(sp)
csrw sstatus, t0
csrw sepc, t1
csrw sscratch, t2
```

**回答：**

处理`sstatus`、`sepc`、`sscratch`。它们必须安排在恢复通用寄存器**之前**，原因有两点：一是 L58 释放 TrapContext、L60 换栈之后，内核栈上的这些数据就再也读不到了；二是 `sscratch` 必须在 L60 那条换栈指令之前准备就绪。

| 寄存器 | 数据来源 | 对于进入用户态的意义 |
| --- | --- | --- |
| `sstatus` | `TrapContext.sstatus`，偏移 `32*8` | 恢复用户程序的处理器状态字。其中 **SPP** 决定 `sret` 之后回到哪个特权级——首次进入时由 `sstatus.set_spp(SPP::User)` 显式设为 User（`os/src/trap/context.rs:23`）；**SPIE** 决定返回后是否开启中断；`FS`/`SUM` 等字段也一并还原为用户程序的值。 |
| `sepc` | `TrapContext.sepc`，偏移 `33*8` | `sret` 会把 PC 置为 `sepc`，即用户程序恢复执行的地址。首次进入时它是 app 入口 `get_base_i(app_id)`；syscall 返回时 `trap_handler` 已 `cx.sepc += 4`，使其指向 `ecall` 的**下一条**指令，避免死循环。 |
| `sscratch` | `TrapContext.x[2]`（用户栈），偏移 `2*8` | 暂存**用户栈指针**，供 L60 的 `csrrw sp, sscratch, sp` 原子换栈使用。这是内核约定：S 态下 `sscratch` 存用户栈，`sp` 指向内核栈。 |


3. L50-L56：为何跳过了 x2 和 x4？
```c++
ld x1, 1*8(sp)
ld x3, 3*8(sp)
.set n, 5
.rept 27
   LOAD_GP %n
   .set n, n+1
.endr
```

**回答：**

- **跳过 x0**：`x0` 硬连线为常量 0，既不能写也无需恢复，所以循环从 `x1` 开始（`.set n, 5` 之前先单独恢复 `x1`、`x3`）。
- **跳过 x2（sp）**：这是最关键的一点。如果在这里用 `ld x2, 2*8(sp)` 恢复 sp，那么紧接着的 `addi sp, sp, 34*8`（释放 TrapContext）和 `csrrw sp, sscratch, sp`（换栈）就会作用在**用户栈**上——内核栈上的 TrapContext 无法正确释放，内核还会跑到用户栈上运行。所以 sp 必须一直留在内核栈，直到 L60 由 `csrrw` 与 `sscratch` 原子交换才拿到用户栈值。
- **跳过 x4（tp）**：`tp` 是线程指针，用于线程局部存储。本章的用户程序不使用 TLS，内核也不需要为每个任务维护不同的 `tp`，保存/恢复它没有意义，源码注释即写作 `# skip tp(x4), application does not use it`。因此 `TrapContext.x[4]` 恒为 0。
- **为什么是 `.set n, 5` + `.rept 27`**：覆盖 `x5`~`x31` 共 27 个寄存器；再加上单独恢复的 `x1`、`x3`、由 `sscratch` 间接恢复的 `x2`、以及无需处理的 `x0` 与 `x4`，32 个通用寄存器就全部安排完毕了。

4. L60：该指令之后，sp 和 sscratch 中的值分别有什么意义？
```c++
csrrw sp, sscratch, sp
```

**回答：**

`csrrw rd, csr, rs1` 的语义是**原子地**执行 `rd ← csr_old; csr ← rs1`。因此 L60 执行后：

- **sp = 原来的 sscratch = 用户栈指针**（即 `TrapContext.x[2]`，由 L48 写入）——从这一刻起 CPU 改用用户栈；
- **sscratch = 原来的 sp = 内核栈顶**（即 `KERNEL_STACK[i].get_sp()`，已由 L58 的 `addi sp, sp, 34*8` 释放掉 TrapContext）。

意义：用一条原子指令完成"内核栈 → 用户栈"的切换，同时把**内核栈顶**暂存进 `sscratch`。这正是下一次 trap 进入 `__alltraps` 时，L13 那条 `csrrw` 能立刻拿到内核栈的前提。

由此得到贯穿整个 trap 机制的约定：**U 态执行时 `sscratch` 存内核栈顶，S 态执行时 `sscratch` 存用户栈指针**；`sp` 始终指向当前特权级正在使用的那一个栈。`sscratch` 相当于两个栈之间的"交换中转站"。

5. __restore：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？

**回答：**

状态切换发生在 **`sret`（`os/src/trap/trap.S:61`）** 这一条指令。

`sret` 由硬件一次性完成三件事：把特权级置为 `sstatus.SPP` 所指示的模式；把 `sstatus.SIE` 置为 `sstatus.SPIE` 并把 `SPIE` 置 1（恢复中断开关状态）；把 PC 置为 `sepc`。

之所以执行后进入 U 态，是因为紧邻的 L46 刚写入 `sstatus` 的值中 **SPP = User**：
- 首次进入用户态时，`TrapContext::app_init_context` 显式调用了 `sstatus.set_spp(SPP::User)`（`os/src/trap/context.rs:23`）；
- 若是 trap 返回，则 `__alltraps` 陷入 S 态时硬件已把 `SPP` 置为 User，并被原样保存在 TrapContext 中。

因此 `sret` 之后处理器从 **S 态降到 U 态**，并从 `sepc` 所指地址继续执行。

到 `sret` 之前，所有准备工作都已完成：`sepc`/`sstatus`/`sscratch` 就位（L43-48）、通用寄存器恢复（L50-56）、TrapContext 释放（L58）、sp 换成用户栈（L60）。所以用户程序看到的是完全连续、仿佛从未被打断过的现场。

6. L13：该指令之后，sp 和 sscratch 中的值分别有什么意义？
```c++
csrrw sp, sscratch, sp
```

**回答：**

进入 `__alltraps` 时 CPU 刚刚由 U 态陷入 S 态，此时 **sp = 用户栈指针**，**sscratch = 内核栈顶**（这是 U 态下的约定）。执行 `csrrw sp, sscratch, sp` 之后：

- **sp = 原来的 sscratch = 内核栈顶**（`KERNEL_STACK[i].get_sp()`）——从此内核在自己专属的栈上工作，不再触碰用户栈；
- **sscratch = 原来的 sp = 用户栈指针**——把用户栈指针暂存起来，随后 L34-35 的 `csrr t2, sscratch; sd t2, 2*8(sp)` 把它写进 TrapContext 的 `x[2]` 槽位长期保存。

意义：一条原子指令既完成了"用户栈 → 内核栈"的切换，又安全地保住了用户栈指针，避免在保存现场的过程中污染用户栈。这也是紧接着 L16 `addi sp, sp, -34*8` 能安全地在内核栈上分配 TrapContext 的前提——此时 sp 已经指向内核栈顶。

7. 从 U 态进入 S 态是哪一条指令发生的？

**回答：**

**不在 `trap.S` 中的任何一条指令。** U 态 → S 态的切换由**硬件**在用户程序执行 `ecall`、或发生异常/中断的那一刻自动完成，随后 PC 被置为 `stvec` 所指向的 `__alltraps`。换言之，`__alltraps` 的第一条指令开始执行时，CPU **已经在 S 态**了，`trap.S` 只是接手后续工作。

具体有两类路径：

1. **系统调用**：用户态执行 `ecall` → 触发 "Environment call from U-mode" 异常。硬件自动完成：特权级 U→S；`sepc` ← 该 `ecall` 指令的 PC；`scause` ← `Exception::UserEnvCall`；`sstatus.SPP` ← User；PC ← `stvec`。内核在 `trap::init()` 中通过 `stvec::write(__alltraps as usize, TrapMode::Direct)` 把 `stvec` 设为 `__alltraps`（`os/src/trap/mod.rs:30-37`），采用 Direct 模式，所有 trap 都跳到同一个入口。
2. **异常 / 中断**：例如访问非法地址（`StoreFault`）、执行特权指令或访问特权 CSR（`IllegalInstruction`），以及 `trap::enable_timer_interrupt()` 通过 `sie::set_stimer()` 打开之后的**时钟中断**（ch3 用它实现抢占式调度，见 `os/src/trap/mod.rs:67-70`）。这些同样由硬件把特权级提升到 S 并跳转到 `stvec`。

小结：`trap.S` 只负责"换栈 + 保存/恢复现场"，从不负责特权级切换——**进入 S 态靠硬件陷阱（U→S），返回 U 态靠 `sret`（S→U）**。二者构成一对对称的机制，而 `sscratch` 正是支撑这对机制的栈交换中转站。


# 荣誉规则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 deepseek，chatgpt等网页ai 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：均为代码解读，包括作业中的trap.s文件，，帮助我快速理解框架设计

2. 此外，我未参考要求外的资料 ，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。 我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。