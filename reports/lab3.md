1、systemcall
    map、unmap、gettime就不说了

1.1、首先是spawn，提示说不用进行fork，但是可以参考fork的逻辑，唯一的区别就是memory_set直接从elf从头构建即可(记得加入task)
1.2、stride算法的实现比较简单，主要是引入stride后，fork，fork的tcb的stride进度还需要复制吗，(暂时定的是需要复制)

答：建议“复制”（你现在定的方案是对的）。原因：如果 fork 时把子进程 stride 重置为 0，而父进程 stride 已经很大（例如 255），子进程就会比父进程“超前”很多，从而连续被调度、获得不公平的时间片；而复制父进程的 stride（以及 priority），父子从同一位置出发，二者 stride 相等、按队列顺序交替执行，最公平也最简单。唯一需要注意的副作用是子进程继承了父进程可能较大的 stride 值，但只要配合下面的“环形比较器 + 优先级≥2 的窗口上界”，溢出比较仍然正确，无需担心。


2、问答
stride 算法原理非常简单，但是有一个比较大的问题。例如两个 pass = 10 的进程，使用 8bit 无符号整形储存 stride， p1.stride = 255, p2.stride = 250，在 p2 执行一个时间片后，理论上下一次应该 p1 执行。

实际情况是轮到 p1 执行吗？为什么？

答：实际情况**不会**轮到 p1，而是 p2 会再次被调度。原因：8 位无符号 stride 会发生回绕（overflow）。p2 执行一个时间片后 stride 增加 pass = 10，即 250 + 10 = 260，超过 8 位最大值 255，回绕成 4（260 mod 256 = 4）。此时 p1.stride = 255、p2.stride = 4，用普通整数比较 4 < 255，调度器仍认为 p2 是“最小”的，于是再次选中 p2。这与“理论上（无限精度，不溢出时 260 > 255，下一次该 p1 执行）”相矛盾，结果是 p2 连续执行、p1 被饿死。这正是必须引入“环形比较器”的原因——不能再用裸的 `<`/`>` 比较 stride。

我们之前要求进程优先级 >= 2 其实就是为了解决这个问题。可以证明， 在不考虑溢出的情况下 , 在进程优先级全部 >= 2 的情况下，如果严格按照算法执行，那么 STRIDE_MAX – STRIDE_MIN <= BigStride / 2。

为什么？尝试简单说明（不要求严格证明）。

答：每次调度都选中当前 stride 最小的进程，它执行后 stride 增加 pass_i = BigStride / priority_i。因为所有 priority_i ≥ 2，所以 pass_i ≤ BigStride/2。用归纳法可证窗口上界始终成立：

- 初始时所有进程 stride 相同（例如都为 0），差值 0 ≤ BigStride/2。
- 假设某时刻最小值为 m、最大值为 M，且 M − m ≤ BigStride/2。取出 stride = m 的进程执行，其 stride 变为 m + pass_min，其中 pass_min ≤ BigStride/2。
- 新集合 = {m + pass_min} ∪ 其余进程（其余进程 stride 仍落在 [m, M]）：
  - 若 m + pass_min ≤ M：新最大值仍是 M，新最小值 ≥ 旧的次小 ≥ m，故新差值 ≤ M − m ≤ BigStride/2。
  - 若 m + pass_min > M：新最大值为 m + pass_min，新最小值 ≥ m，故新差值 ≤ (m + pass_min) − m = pass_min ≤ BigStride/2。

因此对任何时刻都有 STRIDE_MAX − STRIDE_MIN ≤ BigStride/2。其意义在于：只要用位数足够的整数（如 u64，且 BigStride ≤ 2^64 ⇒ BigStride/2 ≤ 2^63）存放 stride，所有“真实” stride 都落在长度不超过 BigStride/2 的窗口内，彼此在环上的距离都小于半环（2^63），于是下面的环形比较器在即使发生回绕时，也能选出真正最小的那个 stride。

已知以上结论，考虑溢出的情况下，可以为 Stride 设计特别的比较器，让 BinaryHeap<Stride> 的 pop 方法能返回真正最小的 Stride。补全下列代码中的 partial_cmp 函数，假设两个 Stride 永远不会相等。


use core::cmp::Ordering;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // 两个 stride 是 2^64 环上的值；用环上的“前向距离”判断谁更靠前(更小)。
        // self 比 other 更靠前/更小（即 self 更该被调度）当且仅当
        // (other - self) mod 2^64 落在 (0, 2^63] 内（前向距离不超过半环）。
        // 题目约定两个 Stride 永不相等，故不会取到 0。
        let dist = other.0.wrapping_sub(self.0);
        if dist < (1u64 << 63) {
            // self 是真正更小的 stride。但 BinaryHeap 是 max-heap，pop 返回“最大”者，
            // 故把真正最小的说成“最大(Greater)”，这样 pop 才能取出真正最小的 Stride。
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}

// 说明：上面的 partial_cmp 刻意“反转”了比较结果——真正最小的 stride 被报告为
// Ordering::Greater。因为 Rust 的 BinaryHeap 是最大堆，pop() 总是弹出“最大”的元素，
// 只有在最小者被视作“最大”时，pop() 才会返回真正最小的 Stride。
// 仅当 BigStride/2 ≤ 2^63（优先级 ≥ 2 已保证窗口上界 ≤ BigStride/2）时该比较器与
// 无限精度下的正确顺序一致，从而安全处理 stride 回绕。