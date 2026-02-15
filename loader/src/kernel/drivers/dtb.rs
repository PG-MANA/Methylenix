//!
//! Device Tree Blob
//!

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
    base_address: usize,
}

pub struct DtbNodeInfo {
    base_address: usize,
    address_cells: u32,
    size_cells: u32,
}

pub struct DtbPropertyInfo {
    base_address: usize,
    address_cells: u32,
    size_cells: u32,
    len: u32,
}

pub struct SubNodeIter<'a> {
    dtb_manager: &'a DtbManager,
    node_info: Option<DtbNodeInfo>,
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

    pub fn new(base_address: usize) -> Option<Self> {
        let fdt_header = unsafe { &*(base_address as *const FdtHeader) };
        if u32::from_be(fdt_header.magic).to_be_bytes() != Self::DTB_MAGIC {
            pr_err!("Invalid DTB magic");
            return None;
        }
        if u32::from_be(fdt_header.version) > Self::DTB_VERSION {
            pr_err!(
                "Unsupported DTB version: {}",
                u32::from_be(fdt_header.version)
            );
            return None;
        }

        Some(Self { base_address })
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
        let mut p = self.get_string_offset() + (name_offset as usize);
        for c in name {
            if *c != unsafe { *(p as *const u8) } {
                return Ok(false);
            }
            p += 1;
        }
        let l = unsafe { *(p as *const u8) };
        for e in delimiter.iter().chain(b"\0") {
            if *e == l {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn compare_string(
        &self,
        pointer: &mut usize,
        name: &[u8],
        delimiter: &[u8],
    ) -> Result<bool, ()> {
        let skip = |p: &mut usize| {
            while unsafe { *(*p as *const u8) } != b'\0' {
                *p += 1;
            }
            *p += 1; /* Move next of '\0' */
            self.skip_padding(p);
        };

        for c in name {
            if *c != unsafe { *(*pointer as *const u8) } {
                skip(pointer);
                return Ok(false);
            }
            *pointer += 1;
        }
        let l = unsafe { *(*pointer as *const u8) };
        for e in delimiter.iter().chain(b"\0") {
            if *e == l {
                skip(pointer);
                return Ok(true);
            }
        }
        skip(pointer);
        Ok(false)
    }

    fn get_struct_offset(&self) -> usize {
        self.base_address
            + (u32::from_be(unsafe { &*(self.base_address as *const FdtHeader) }.off_dt_struct)
                as usize)
    }

    fn get_struct_size(&self) -> usize {
        u32::from_be(unsafe { &*(self.base_address as *const FdtHeader) }.size_dt_struct) as usize
    }

    fn get_string_offset(&self) -> usize {
        self.base_address
            + (u32::from_be(unsafe { &*(self.base_address as *const FdtHeader) }.off_dt_strings)
                as usize)
    }

    fn get_string_size(&self) -> u32 {
        u32::from_be(unsafe { &*(self.base_address as *const FdtHeader) }.size_dt_strings)
    }

    fn read_node(&self, address: usize) -> Result<&[u8; Self::FDT_NODE_BYTE], ()> {
        if address >= (self.get_struct_offset() + self.get_struct_size()) {
            Err(())
        } else {
            Ok(unsafe { &*(address as *const [u8; Self::FDT_NODE_BYTE]) })
        }
    }

    fn skip_nop(&self, pointer: &mut usize) -> Result<(), ()> {
        while *self.read_node(*pointer)? == Self::FDT_NOP {
            *pointer += Self::FDT_NODE_BYTE;
        }
        Ok(())
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
        let n = *self.read_node(*pointer)?;
        if n != Self::FDT_BEGIN_NODE {
            if n == Self::FDT_END_NODE || n == Self::FDT_END {
                return Ok(None);
            }
            pr_err!(
                "Expected {:#X}, But found {:#X}",
                u32::from_be_bytes(Self::FDT_BEGIN_NODE),
                u32::from_be_bytes(*self.read_node(*pointer)?)
            );
            return Err(());
        }
        *pointer += Self::FDT_NODE_BYTE;
        if self.compare_string(pointer, node_name, b"@")? {
            return Ok(Some(DtbNodeInfo {
                base_address: *pointer,
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
                    *pointer += size_of::<u32>();
                    let name_segment = u32::from_be_bytes(*self.read_node(*pointer)?);
                    *pointer += size_of::<u32>();
                    self.check_address_and_size_cells(
                        name_segment,
                        *pointer,
                        &mut address_cells,
                        &mut size_cells,
                    )?;
                    *pointer += len as usize;
                }
                n => {
                    pr_err!(
                        "Unknown Token(Offset: {:#X}): {:#X}",
                        *pointer - self.base_address,
                        u32::from_be_bytes(n)
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
                    *pointer += size_of::<u32>();
                    /* Skip Name Segment */
                    *pointer += size_of::<u32>();
                    *pointer += len as usize;
                }
                n => {
                    pr_err!(
                        "Unknown Token(Offset: {:#X}): {:#X}",
                        *pointer - self.base_address,
                        u32::from_be_bytes(n)
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
        let struct_base = self.get_struct_offset();

        let (mut pointer, address_cells, size_cells) = if let Some(c) = current_node {
            let mut p = c.base_address;
            if self._skip_to_next_node(&mut p).is_err() {
                return None;
            }
            (p, c.address_cells, c.size_cells)
        } else {
            (
                struct_base,
                Self::DEFAULT_ADDRESS_CELLS,
                Self::DEFAULT_SIZE_CELLS,
            )
        };
        while self.read_node(pointer).is_ok() {
            match self._search_node(node_name, &mut pointer, address_cells, size_cells) {
                Ok(Some(n)) => return Some(n),
                Ok(None) => {
                    match self.read_node(pointer).map(|n| *n) {
                        Ok(Self::FDT_END) | Err(_) => {
                            return None;
                        }
                        Ok(Self::FDT_BEGIN_NODE) => { /* Continue */ }
                        Ok(Self::FDT_END_NODE) | Ok(Self::FDT_NOP) => {
                            pointer += Self::FDT_NODE_BYTE;
                            /* Continue */
                        }
                        Ok(n) => {
                            pr_err!("Unexpected Node: {:#X}", u32::from_be_bytes(n));
                            return None;
                        }
                    }
                }
                Err(()) => return None,
            }
        }
        None
    }

    pub fn iterate_sub_node(&self, node: &DtbNodeInfo) -> Option<SubNodeIter<'_>> {
        let mut pointer = node.base_address;
        let mut address_cells = node.address_cells;
        let mut size_cells = node.size_cells;

        loop {
            self.skip_padding(&mut pointer);
            self.skip_nop(&mut pointer).ok()?;
            match *self.read_node(pointer).ok()? {
                Self::FDT_BEGIN_NODE => {
                    pointer += Self::FDT_NODE_BYTE;
                    self.compare_string(&mut pointer, b"", b"@").ok()?;

                    return Some(SubNodeIter {
                        dtb_manager: self,
                        node_info: Some(DtbNodeInfo {
                            base_address: pointer,
                            address_cells,
                            size_cells,
                        }),
                    });
                }

                Self::FDT_END => {
                    return None;
                }
                Self::FDT_END_NODE => {
                    return None;
                }
                Self::FDT_PROP => {
                    pointer += Self::FDT_NODE_BYTE;
                    let len = u32::from_be_bytes(*self.read_node(pointer).ok()?);
                    pointer += size_of::<u32>();
                    let name_segment = u32::from_be_bytes(*self.read_node(pointer).ok()?);
                    pointer += size_of::<u32>();
                    self.check_address_and_size_cells(
                        name_segment,
                        pointer,
                        &mut address_cells,
                        &mut size_cells,
                    )
                    .ok()?;
                    pointer += len as usize;
                }
                n => {
                    pr_err!(
                        "Unknown Token(Offset: {:#X}): {:#X}",
                        pointer - self.base_address,
                        u32::from_be_bytes(n)
                    );
                    return None;
                }
            }
        }
    }

    pub fn get_property(
        &self,
        node: &DtbNodeInfo,
        property_name: &[u8],
    ) -> Option<DtbPropertyInfo> {
        let mut p = node.base_address;
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
                    p += size_of::<u32>();
                    let name_segment = u32::from_be_bytes(*self.read_node(p).ok()?);
                    p += size_of::<u32>();
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
                            base_address: p,
                            address_cells,
                            size_cells,
                            len,
                        });
                    }
                    p += len as usize;
                }
                n => {
                    pr_err!(
                        "Unknown Token(Offset: {:#X}): {:#X}",
                        p - self.base_address,
                        u32::from_be_bytes(n)
                    );
                    return None;
                }
            }
        }
    }

    pub fn is_node_operational(&self, node: &DtbNodeInfo) -> bool {
        self.get_property(node, &Self::PROP_STATUS)
            .map(|p| unsafe {
                core::slice::from_raw_parts(p.base_address as *const u8, p.len as usize)
                    == Self::PROP_STATUS_OKAY
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
                if unsafe { *((info.base_address + (p as usize)) as *const u8) } == b'\0' {
                    skip = false;
                }
                p += 1;
                continue;
            }
            for c in compatible.iter().chain(b"\0") {
                if unsafe { *((info.base_address + (p as usize)) as *const u8) } != *c {
                    skip = true;
                    continue 'outer;
                }
                p += 1;
            }
            return true;
        }
        false
    }

    pub fn read_reg_property(&self, node: &DtbNodeInfo, index: usize) -> Option<(usize, usize)> {
        let Some(info) = self.get_property(node, &Self::PROP_REG) else {
            return None;
        };
        let mut address: usize = 0;
        let mut size: usize = 0;
        let offset =
            ((info.address_cells + info.size_cells) as usize) * Self::FDT_NODE_BYTE * index;
        if offset + ((info.address_cells + info.size_cells) as usize) * Self::FDT_NODE_BYTE
            > info.len as usize
        {
            return None;
        }

        let mut p = info.base_address + offset;
        for _ in 0..(info.address_cells as usize * Self::FDT_NODE_BYTE) {
            address <<= 8;
            address |= unsafe { *(p as *const u8) } as usize;
            p += 1;
        }
        for _ in 0..(info.size_cells as usize * Self::FDT_NODE_BYTE) {
            size <<= 8;
            size |= unsafe { *(p as *const u8) } as usize;
            p += 1;
        }
        Some((address, size))
    }

    pub fn read_property_as_u8_array(&self, info: &DtbPropertyInfo) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                info.base_address as *const u8,
                (info.len as usize) / size_of::<u8>(),
            )
        }
    }

    pub fn read_property_as_u32(&self, info: &DtbPropertyInfo, index: usize) -> Option<u32> {
        if (info.len as usize) < (size_of::<u32>() * (index + 1)) {
            None
        } else {
            Some(u32::from_be(unsafe {
                *((info.base_address + size_of::<u32>() * index) as *const u32)
            }))
        }
    }
}

impl<'a> Iterator for SubNodeIter<'a> {
    type Item = DtbNodeInfo;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(current) = &self.node_info else {
            return None;
        };
        let mut next_pointer = current.base_address;
        let next_node;

        let _ = self.dtb_manager._skip_to_next_node(&mut next_pointer);
        if self
            .dtb_manager
            .read_node(next_pointer)
            .map(|n| *n == DtbManager::FDT_BEGIN_NODE)
            .unwrap_or(false)
        {
            next_pointer += DtbManager::FDT_NODE_BYTE;
            let _ = self
                .dtb_manager
                .compare_string(&mut next_pointer, b"", b"@"); // Skip name
            next_node = Some(DtbNodeInfo {
                base_address: next_pointer,
                address_cells: current.address_cells,
                size_cells: current.size_cells,
            });
        } else {
            next_node = None;
        }
        core::mem::replace(&mut self.node_info, next_node)
    }
}
