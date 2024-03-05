//!
//! EFI Memory Map
//!

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
#[repr(u32)]
#[allow(dead_code)]
pub enum EfiMemoryType {
    ReservedMemoryType,
    LoaderCode,
    LoaderData,
    BootServicesCode,
    BootServicesData,
    RuntimeServicesCode,
    RuntimeServicesData,
    ConventionalMemory,
    UnusableMemory,
    ACPIReclaimMemory,
    ACPIMemoryNVS,
    MemoryMappedIO,
    MemoryMappedIOPortSpace,
    PalCode,
    PersistentMemory,
    MaxMemoryType,
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug)]
#[repr(u32)]
#[allow(dead_code)]
pub enum EfiAllocateType {
    AllocateAnyPages,
    AllocateMaxAddress,
    AllocateAddress,
    MaxAllocateType,
}

impl core::fmt::Display for EfiMemoryType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
        f.write_fmt(format_args!("{:?}", self))
    }
}
