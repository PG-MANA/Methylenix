//!
//! EFI Memory Map
//!

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
#[repr(u32)]
pub enum EfiMemoryType {
    EfiReservedMemoryType,
    EfiLoaderCode,
    EfiLoaderData,
    EfiBootServicesCode,
    EfiBootServicesData,
    EfiRuntimeServicesCode,
    EfiRuntimeServicesData,
    EfiConventionalMemory,
    EfiUnusableMemory,
    EfiACPIReclaimMemory,
    EfiACPIMemoryNVS,
    EfiMemoryMappedIO,
    EfiMemoryMappedIOPortSpace,
    EfiPalCode,
    EfiPersistentMemory,
    EfiMaxMemoryType,
}

impl core::fmt::Display for EfiMemoryType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        f.write_fmt(format_args!("{:?}", self))
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
#[repr(u64)]
pub enum EfiMemoryAttribute {
    EfiMemoryUc = 0x0000000000000001,
    EfiMemoryWc = 0x0000000000000002,
    EfiMemoryWt = 0x0000000000000004,
    EfiMemoryWb = 0x0000000000000008,
    EfiMemoryUce = 0x0000000000000010,
    EfiMemoryWp = 0x0000000000001000,
    EfiMemoryRp = 0x0000000000002000,
    EfiMemoryXp = 0x0000000000004000,
    EfiMemoryNv = 0x0000000000008000,
    EfiMemoryMoreReliable = 0x0000000000010000,
    EfiMemoryRo = 0x0000000000020000,
    EfiMemorySp = 0x0000000000040000,
    EfiMemoryCpuCrypto = 0x0000000000080000,
    EfiMemoryRuntime = 0x8000000000000000,
}

#[derive(Clone)]
#[repr(C)]
pub struct EfiMemoryDescriptor {
    pub memory_type: EfiMemoryType,
    pub physical_start: usize,
    pub virtual_start: usize,
    pub number_of_pages: u64,
    pub attribute: EfiMemoryAttribute,
}
