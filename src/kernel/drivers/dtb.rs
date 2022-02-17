//!
//! Device Tree Blob
//!

use crate::arch::target_arch::paging::PAGE_SIZE;

use crate::kernel::memory_manager::data_type::{
    Address, MSize, MemoryPermissionFlags, PAddress, VAddress,
};
use crate::{free_pages, io_remap, mremap};

#[repr(C)]
struct FdtHeader {
    magic: u32,
    total_size: u32,
    off_dt_struct: u32,
    off_dt_strings: u32,
    off_mem_reserved_map: u32,
    version: u32,
    last_comp_version: u32,
    boot_cpuid_phys: u32,
    size_dt_strings: u32,
    size_dt_struct: u32,
}

pub struct DtbManager {
    base_address: VAddress,
}

pub struct DtbNodeInfo {
    base_address: VAddress,
    address_cells: u32,
    size_cells: u32,
}

pub struct DtbPropertyInfo {
    base_address: VAddress,
    address_cells: u32,
    size_cells: u32,
    len: u32,
}

impl DtbManager {
    const DTB_MAGIC: [u8; 4] = [0xd0, 0x0d, 0xfe, 0xed];
    const DTB_VERSION: u32 = 17;
    const FDT_NODE_BYTE: usize = 0x04;
    const FDT_BEGIN_NODE: [u8; Self::FDT_NODE_BYTE] = [0x00, 0x00, 0x00, 0x01];
    const FDT_END_NODE: [u8; Self::FDT_NODE_BYTE] = [0x00, 0x00, 0x00, 0x02];
    const FDT_PROP: [u8; Self::FDT_NODE_BYTE] = [0x00, 0x00, 0x00, 0x03];
    const FDT_NOP: [u8; Self::FDT_NODE_BYTE] = [0x00, 0x00, 0x00, 0x04];
    const FDT_END: [u8; Self::FDT_NODE_BYTE] = [0x00, 0x00, 0x00, 0x09];

    const PROP_ADDRESS_CELLS: [u8; 14] = *b"#address-cells";
    const PROP_SIZE_CELLS: [u8; 11] = *b"#size-cells";
    const PROP_REG: [u8; 3] = *b"reg";
    const PROP_STATUS: [u8; 6] = *b"status";
    const PROP_STATUS_OKAY: [u8; 5] = *b"okay\0";
    const PROP_COMPATIBLE: [u8; 10] = *b"compatible";
    pub const PROP_INTERRUPTS: [u8; 10] = *b"interrupts";

    const DEFAULT_ADDRESS_CELLS: u32 = 2;
    const DEFAULT_SIZE_CELLS: u32 = 1;

    pub fn new() -> Self {
        Self {
            base_address: VAddress::new(0),
        }
    }

    pub fn init(&mut self, dtb_header_address: PAddress) -> bool {
        const INITIAL_MAP_SIZE: MSize = PAGE_SIZE;
        self.base_address = match io_remap!(
            dtb_header_address,
            INITIAL_MAP_SIZE,
            MemoryPermissionFlags::data()
        ) {
            Ok(v) => v,
            Err(e) => {
                pr_err!("Failed to map DTB: {:?}", e);
                return false;
            }
        };

        let fdt_header = unsafe { &*(self.base_address.to_usize() as *const FdtHeader) };
        if u32::from_be(fdt_header.magic).to_be_bytes() != Self::DTB_MAGIC {
            pr_err!("Invalid DTB magic");
            let _ = free_pages!(self.base_address);
            return false;
        }
        if u32::from_be(fdt_header.version) > Self::DTB_VERSION {
            pr_err!(
                "Unsupported DTB version: {}",
                u32::from_be(fdt_header.version)
            );
            let _ = free_pages!(self.base_address);
            return false;
        }
        if (u32::from_be(fdt_header.total_size) as usize) > INITIAL_MAP_SIZE.to_usize() {
            self.base_address = match mremap!(
                self.base_address,
                INITIAL_MAP_SIZE,
                MSize::new(u32::from_be(fdt_header.total_size) as usize)
            ) {
                Ok(v) => v,
                Err(e) => {
                    pr_err!("Failed to remap DTB: {:?}", e);
                    let _ = free_pages!(self.base_address);
                    return false;
                }
            };
        }
        return true;
    }

    fn compare_name_segment(
        &self,
        name_offset: u32,
        name: &[u8],
        delimiter: &[u8],
    ) -> Result<bool, ()> {
        if name_offset >= self.get_string_size() {
            return Err(());
        }
        let mut p = self.get_string_offset().to_usize() + (name_offset as usize);
        for c in name {
            if *c != unsafe { *(p as *const u8) } {
                return Ok(false);
            }
            p += 1;
        }
        let l = unsafe { *(p as *const u8) };
        for e in delimiter.iter().chain(&[b'\0']) {
            if *e == l {
                return Ok(true);
            }
        }
        return Ok(false);
    }

    fn compare_string(
        &self,
        pointer: &mut usize,
        name: &[u8],
        delimiter: &[u8],
    ) -> Result<bool, ()> {
        for c in name {
            if *c != unsafe { *(*pointer as *const u8) } {
                return Ok(false);
            }
            *pointer += 1;
        }
        let l = unsafe { *(*pointer as *const u8) };
        for e in delimiter.iter().chain(&[b'\0']) {
            if *e == l {
                while unsafe { *(*pointer as *const u8) } != b'\0' {
                    *pointer += 1;
                }
                self.skip_padding(pointer);
                return Ok(true);
            }
        }
        while unsafe { *(*pointer as *const u8) } != b'\0' {
            *pointer += 1;
        }
        self.skip_padding(pointer);
        return Ok(false);
    }

    fn get_struct_offset(&self) -> VAddress {
        self.base_address
            + MSize::new(u32::from_be(
                unsafe { &*(self.base_address.to_usize() as *const FdtHeader) }.off_dt_struct,
            ) as usize)
    }

    fn get_struct_size(&self) -> MSize {
        MSize::new(u32::from_be(
            unsafe { &*(self.base_address.to_usize() as *const FdtHeader) }.size_dt_struct,
        ) as usize)
    }

    fn get_string_offset(&self) -> VAddress {
        self.base_address
            + MSize::new(u32::from_be(
                unsafe { &*(self.base_address.to_usize() as *const FdtHeader) }.off_dt_strings,
            ) as usize)
    }

    fn get_string_size(&self) -> u32 {
        u32::from_be(
            unsafe { &*(self.base_address.to_usize() as *const FdtHeader) }.size_dt_strings,
        )
    }

    fn read_node(&self, address: usize) -> Result<&[u8; Self::FDT_NODE_BYTE], ()> {
        if address >= (self.get_struct_offset() + self.get_struct_size()).to_usize() {
            Err(())
        } else {
            Ok(unsafe { &*(address as *const [u8; Self::FDT_NODE_BYTE]) })
        }
    }

    fn skip_nop(&self, pointer: &mut usize) -> Result<(), ()> {
        while *self.read_node(*pointer)? == Self::FDT_NOP {
            *pointer += Self::FDT_NODE_BYTE;
        }
        return Ok(());
    }

    fn skip_padding(&self, pointer: &mut usize) {
        *pointer = ((*pointer - 1) & !(Self::FDT_NODE_BYTE - 1)) + Self::FDT_NODE_BYTE;
    }

    fn check_address_and_size_cells(
        &self,
        name_segment: u32,
        pointer: usize,
        address_cells: &mut u32,
        size_cells: &mut u32,
    ) -> Result<(), ()> {
        if self.compare_name_segment(name_segment, &Self::PROP_ADDRESS_CELLS, &[])? {
            *address_cells = u32::from_be_bytes(*self.read_node(pointer)?);
        } else if self.compare_name_segment(name_segment, &Self::PROP_SIZE_CELLS, &[])? {
            *size_cells = u32::from_be_bytes(*self.read_node(pointer)?);
        }
        Ok(())
    }

    fn _search_node(
        &self,
        node_name: &[u8],
        pointer: &mut usize,
        mut address_cells: u32,
        mut size_cells: u32,
    ) -> Result<Option<DtbNodeInfo>, ()> {
        self.skip_nop(pointer)?;
        if *self.read_node(*pointer)? != Self::FDT_BEGIN_NODE {
            pr_err!("Invalid DTB");
            return Err(());
        }
        *pointer += Self::FDT_NODE_BYTE;
        if self.compare_string(pointer, node_name, &[b'@'])? {
            return Ok(Some(DtbNodeInfo {
                base_address: VAddress::new(*pointer),
                address_cells,
                size_cells,
            }));
        }
        loop {
            self.skip_padding(pointer);
            self.skip_nop(pointer)?;
            match *self.read_node(*pointer)? {
                Self::FDT_BEGIN_NODE => {
                    if let Some(i) =
                        self._search_node(node_name, pointer, address_cells, size_cells)?
                    {
                        return Ok(Some(i));
                    }
                }
                Self::FDT_END => {
                    return Err(());
                }
                Self::FDT_END_NODE => {
                    *pointer += Self::FDT_NODE_BYTE;
                    return Ok(None);
                }
                Self::FDT_PROP => {
                    *pointer += Self::FDT_NODE_BYTE;
                    let len = u32::from_be_bytes(*self.read_node(*pointer)?);
                    *pointer += core::mem::size_of::<u32>();
                    let name_segment = u32::from_be_bytes(*self.read_node(*pointer)?);
                    *pointer += core::mem::size_of::<u32>();
                    self.check_address_and_size_cells(
                        name_segment,
                        *pointer,
                        &mut address_cells,
                        &mut size_cells,
                    )?;
                    *pointer += len as usize;
                }
                _ => {
                    pr_err!(
                        "Unknown Token: {:#X}",
                        u32::from_be_bytes(*self.read_node(*pointer)?)
                    );
                    return Err(());
                }
            }
        }
    }

    fn _skip_to_next_node(&self, pointer: &mut usize) -> Result<(), ()> {
        loop {
            self.skip_padding(pointer);
            self.skip_nop(pointer)?;
            match *self.read_node(*pointer)? {
                Self::FDT_BEGIN_NODE => {
                    *pointer += Self::FDT_NODE_BYTE;
                    self._skip_to_next_node(pointer)?;
                }
                Self::FDT_END => {
                    return Err(());
                }
                Self::FDT_END_NODE => {
                    *pointer += Self::FDT_NODE_BYTE;
                    return Ok(());
                }
                Self::FDT_PROP => {
                    *pointer += Self::FDT_NODE_BYTE;
                    let len = u32::from_be_bytes(*self.read_node(*pointer)?);
                    *pointer += core::mem::size_of::<u32>();
                    /* Skip Name Segment */
                    *pointer += core::mem::size_of::<u32>();
                    *pointer += len as usize;
                }
                _ => {
                    pr_err!(
                        "Unknown Token: {:#X}",
                        u32::from_be_bytes(*self.read_node(*pointer)?)
                    );
                    return Err(());
                }
            }
        }
    }

    pub fn search_node(
        &self,
        node_name: &[u8],
        current_node: Option<&DtbNodeInfo>,
    ) -> Option<DtbNodeInfo> {
        if self.base_address.is_zero() {
            return None;
        }
        let struct_base = self.get_struct_offset();

        let (mut pointer, address_cells, size_cells) = if let Some(c) = current_node {
            let mut p = c.base_address.to_usize();
            if self._skip_to_next_node(&mut p).is_err() {
                return None;
            }
            (p, c.address_cells, c.size_cells)
        } else {
            (
                struct_base.to_usize(),
                Self::DEFAULT_ADDRESS_CELLS,
                Self::DEFAULT_SIZE_CELLS,
            )
        };
        while self.read_node(pointer).is_ok() {
            match self._search_node(node_name, &mut pointer, address_cells, size_cells) {
                Ok(Some(n)) => return Some(n),
                Ok(None) => pointer += Self::FDT_NODE_BYTE,
                Err(()) => return None,
            }
        }
        return None;
    }

    pub fn get_property(
        &self,
        node: &DtbNodeInfo,
        property_name: &[u8],
    ) -> Option<DtbPropertyInfo> {
        if self.base_address.is_zero() {
            return None;
        }
        let mut p = node.base_address.to_usize();
        let mut address_cells = node.address_cells;
        let mut size_cells = node.size_cells;
        loop {
            self.skip_padding(&mut p);
            if self.skip_nop(&mut p).is_err() {
                return None;
            }
            match *self.read_node(p).ok()? {
                Self::FDT_BEGIN_NODE => {
                    return None;
                }
                Self::FDT_END => {
                    return None;
                }
                Self::FDT_END_NODE => {
                    return None;
                }
                Self::FDT_PROP => {
                    p += Self::FDT_NODE_BYTE;
                    let len = u32::from_be_bytes(*self.read_node(p).ok()?);
                    p += core::mem::size_of::<u32>();
                    let name_segment = u32::from_be_bytes(*self.read_node(p).ok()?);
                    p += core::mem::size_of::<u32>();
                    self.check_address_and_size_cells(
                        name_segment,
                        p,
                        &mut address_cells,
                        &mut size_cells,
                    )
                    .ok()?;
                    if self
                        .compare_name_segment(name_segment, property_name, &[])
                        .ok()?
                    {
                        return Some(DtbPropertyInfo {
                            base_address: VAddress::new(p),
                            address_cells,
                            size_cells,
                            len,
                        });
                    }
                    p += len as usize;
                }
                _ => {
                    pr_err!(
                        "Unknown Token: {:#X}",
                        u32::from_be_bytes(*self.read_node(p).ok()?)
                    );
                    return None;
                }
            }
        }
    }

    pub fn is_node_operational(&self, node: &DtbNodeInfo) -> bool {
        self.get_property(node, &Self::PROP_STATUS)
            .and_then(|p| {
                Some(
                    unsafe { *(p.base_address.to_usize() as *const [u8; 5]) }
                        == Self::PROP_STATUS_OKAY,
                )
            })
            .unwrap_or(true)
    }

    pub fn is_device_compatible(&self, node: &DtbNodeInfo, compatible: &[u8]) -> bool {
        let Some(info) = self.get_property(node, &Self::PROP_COMPATIBLE) else {
            return false;
        };
        let mut p = 0;
        let mut skip = false;
        'outer: while p < info.len {
            if skip {
                if unsafe { *((info.base_address.to_usize() + (p as usize)) as *const u8) } == b'\0'
                {
                    skip = false;
                }
                p += 1;
                continue;
            }
            for c in compatible.iter().chain(&[b'\0']) {
                if unsafe { *((info.base_address.to_usize() + (p as usize)) as *const u8) } != *c {
                    skip = true;
                    continue 'outer;
                }
                p += 1;
            }
            return true;
        }
        return false;
    }

    pub fn read_reg_property(&self, node: &DtbNodeInfo, index: usize) -> Option<(usize, usize)> {
        let Some(info) = self.get_property(node, &Self::PROP_REG) else {
            return None;
        };
        let mut address: usize = 0;
        let mut size: usize = 0;
        let offset = ((info.address_cells + info.size_cells) as usize) * index;
        if offset + ((info.address_cells + info.size_cells) as usize) > info.len as usize {
            return None;
        }
        for i in 0..info.address_cells {
            address <<= 8;
            address |=
                unsafe { *((info.base_address.to_usize() + offset + i as usize) as *const u8) }
                    as usize;
        }
        for i in 0..info.size_cells {
            size <<= 8;
            size |= unsafe {
                *((info.base_address.to_usize() + offset + (info.address_cells + i) as usize)
                    as *const u8)
            } as usize;
        }
        Some((address, size))
    }

    pub fn read_property_as_u8_array(&self, info: &DtbPropertyInfo) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                info.base_address.to_usize() as *const u8,
                (info.len as usize) / core::mem::size_of::<u8>(),
            )
        }
    }

    pub fn read_property_as_u32_array(&self, info: &DtbPropertyInfo) -> &[u32] {
        unsafe {
            core::slice::from_raw_parts(
                info.base_address.to_usize() as *const u32,
                (info.len as usize) / core::mem::size_of::<u32>(),
            )
        }
    }

    pub fn read_property_as_u32(&self, info: &DtbPropertyInfo) -> Option<u32> {
        if (info.len as usize) < core::mem::size_of::<u32>() {
            None
        } else {
            Some(unsafe { *(info.base_address.to_usize() as *const u32) })
        }
    }
}
