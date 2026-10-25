//! Implementation of [`PageTableEntry`] and [`PageTable`].

// use crate::mm::page_table;

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        /// Valid
        const V = 1 << 0;
        /// Readable
        const R = 1 << 1;
        /// Writable
        const W = 1 << 2;
        /// eXecutable
        const X = 1 << 3;
        /// User
        const U = 1 << 4;
        /// Global
        const G = 1 << 5;
        /// Accessed
        const A = 1 << 6;
        /// Dirty
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
pub struct PageTableEntry {
    /// bits of page table entry
    pub bits: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    /// Create an empty page table entry
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// Get the physical page number from the page table entry
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /// Get the flags from the page table entry
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /// The page pointered by page table entry is valid?
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is readable?
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is writable?
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is executable?
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// Create a new page table
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /// Find PageTableEntry by VirtPageNum, create a frame for a 4KB page table if not exist
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }
    /// Find PageTableEntry by VirtPageNum
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    /// set the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    /// remove the map between virtual page number and physical page number
    /// 幂等：若页表项不存在或已无效则直接忽略，避免重复 unmap 时 panic。
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        if let Some(pte) = self.find_pte(vpn) {
            if pte.is_valid() {
                *pte = PageTableEntry::empty();
            }
        }
    }
    /// get the page table entry from the virtual page number
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| *pte)
    }
    /// get the token from the page table
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

/// Translate&Copy a ptr[u8] array with LENGTH len to a mutable u8 Vec through page table
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

/// 将src的数据拷贝到用户的虚拟地址dts_va，需要处理跨页和访问异常
/// 要求目标页必须有效、可写且为用户态页，否则返回 Err(()) 而不是越权写入/崩溃
pub unsafe fn copy_to_user<T:Copy>(token: usize, dst: usize, src: &T) -> Result<(), ()>{
    let page_table = PageTable::from_token(token);
    let len = core::mem::size_of::<T>();
    let src_bytes = core::slice::from_raw_parts(src as *const T as *const u8, len);

    let dst_va = VirtAddr::from(dst);
    let mut dst_vpn = dst_va.floor();
    let mut dst_offset = dst_va.page_offset();
    let mut src_offset = 0;

    while src_offset < len {
        let pte = page_table.translate(dst_vpn).ok_or(())?;
        // 必须有效、可写且为用户态页，避免写入内核/只读页
        if !pte.is_valid() || !pte.writable() || pte.flags() & PTEFlags::U == PTEFlags::empty() {
            return Err(());
        }
        let dst_ppn = pte.ppn();
        let dst_bytes = &mut dst_ppn.get_bytes_array()[dst_offset..];

        let copy_len = (len - src_offset).min(dst_bytes.len());

        // 严格匹配两端切片长度，防止 copy_from_slice panic
        dst_bytes[..copy_len].copy_from_slice(&src_bytes[src_offset..src_offset + copy_len]);

        src_offset += copy_len;
        dst_offset = 0; // 后续物理页均从页首 offset 0 开始写入
        dst_vpn.step();
    }
    Ok(())
}

/// 从用户虚拟地址 src 读取长度为 size_of::<T>() 的数据到内核 dst，处理跨页、权限及访问异常。
/// 要求源页必须有效、可读且为用户态页，否则返回 Err(()) 而不会越权读取/崩溃。
pub unsafe fn copy_from_user<T: Copy>(token: usize, dst: &mut T, src: usize) -> Result<(), ()> {
    let page_table = PageTable::from_token(token);
    let len = core::mem::size_of::<T>();
    let dst_bytes = core::slice::from_raw_parts_mut(dst as *mut T as *mut u8, len);

    let src_va = VirtAddr::from(src);
    let mut src_vpn = src_va.floor();
    let mut src_offset = src_va.page_offset();
    let mut dst_offset = 0;

    while dst_offset < len {
        let pte = page_table.translate(src_vpn).ok_or(())?;
        // 必须有效、可读且为用户态页，避免读取内核/不可读页
        if !pte.is_valid() || !pte.readable() || pte.flags() & PTEFlags::U == PTEFlags::empty() {
            return Err(());
        }
        let src_ppn = pte.ppn();
        let src_bytes = &src_ppn.get_bytes_array()[src_offset..];

        let copy_len = (len - dst_offset).min(src_bytes.len());
        // 严格匹配两端切片长度，防止 copy_from_slice panic
        dst_bytes[dst_offset..dst_offset + copy_len]
            .copy_from_slice(&src_bytes[..copy_len]);

        dst_offset += copy_len;
        src_offset = 0; // 后续物理页均从页首 offset 0 开始读取
        src_vpn.step();
    }
    Ok(())
}
