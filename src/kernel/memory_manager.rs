//!
//! Memory Manager
//!
//! This manager is the frontend of the memory allocation.
//! Each process has a memory manager.
//! MemoryManager treats page size allocation, if you want to alloc memory for objects in the kernel,
//! use memory_allocator.
//! In this memory system, you should not use alloc::*, use only core::*
//!

pub mod data_type;
pub mod global_allocator;
pub mod memory_allocator;
pub mod physical_memory_manager;
pub mod slab_allocator;
pub mod system_memory_manager;
pub mod virtual_memory_manager;

use self::data_type::{
    Address, MIndex, MOrder, MPageOrder, MSize, MemoryOptionFlags, MemoryPermissionFlags, PAddress,
    VAddress,
};
use self::physical_memory_manager::PhysicalMemoryManager;
use self::system_memory_manager::get_physical_memory_manager;
use self::virtual_memory_manager::VirtualMemoryManager;

use crate::arch::target_arch::context::memory_layout::physical_address_to_direct_map;
use crate::arch::target_arch::paging::{
    PagingError, NEED_COPY_HIGH_MEMORY_PAGE_TABLE, PAGE_MASK, PAGE_SHIFT, PAGE_SIZE,
    PAGE_SIZE_USIZE,
};

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::task_manager::KERNEL_PID;

pub struct MemoryManager {
    virtual_memory_manager: VirtualMemoryManager,
}

#[derive(Clone, Eq, PartialEq, Copy, Debug)]
pub enum MemoryError {
    NotAligned,
    InvalidSize,
    InvalidAddress,
    AllocAddressFailed,
    FreeAddressFailed,
    AddressNotAvailable,
    MapAddressFailed,
    InternalError,
    EntryPoolRunOut,
    PagingError(PagingError),
}

impl From<PagingError> for MemoryError {
    fn from(e: PagingError) -> Self {
        Self::PagingError(e)
    }
}

impl MemoryManager {
    pub fn new(virtual_memory_manager: VirtualMemoryManager) -> Self {
        Self {
            virtual_memory_manager,
        }
    }

    pub fn is_kernel_memory_manager(&self) -> bool {
        &get_kernel_manager_cluster().kernel_memory_manager as *const _ == self as *const _
    }

    fn clone_kernel_memory_pages(&mut self) -> Result<(), MemoryError> {
        self.virtual_memory_manager.clone_kernel_area(
            &get_kernel_manager_cluster()
                .kernel_memory_manager
                .virtual_memory_manager,
        )
    }

    pub fn clone_kernel_memory_pages_if_needed(&mut self) -> Result<(), MemoryError> {
        self._clone_kernel_memory_pages_if_needed()
    }

    fn _clone_kernel_memory_pages_if_needed(&mut self) -> Result<(), MemoryError> {
        /* Depend on the architecture */
        if !NEED_COPY_HIGH_MEMORY_PAGE_TABLE {
            return Ok(());
        }
        if self.is_kernel_memory_manager()
        /*  is above if expression really necessary? */
        {
            if get_cpu_manager_cluster().run_queue.get_running_pid() == KERNEL_PID {
                return Ok(());
            } /* running thread must be some */
            let running_process = get_cpu_manager_cluster().run_queue.get_running_process();
            unsafe { &mut *running_process.get_memory_manager() }.clone_kernel_memory_pages()
        } else {
            self.clone_kernel_memory_pages()
        }
    }

    fn add_physical_memory_manager_pool(
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<(), MemoryError> {
        let address = pm_manager
            .alloc(PAGE_SIZE, MOrder::new(0))
            .expect("Failed to alloc memory");
        pm_manager.add_memory_entry_pool(
            physical_address_to_direct_map(address).to_usize(),
            PAGE_SIZE_USIZE,
        );
        return Ok(());
    }

    fn allocate_physical_memory(
        size: MSize,
        align_order: MOrder,
        pm_manager: &mut PhysicalMemoryManager,
    ) -> Result<PAddress, MemoryError> {
        match pm_manager.alloc(size, align_order) {
            Ok(physical_address) => Ok(physical_address),
            Err(MemoryError::EntryPoolRunOut) => {
                if let Err(e) = Self::add_physical_memory_manager_pool(pm_manager) {
                    pr_err!(
                        "Failed to add memory pool to PhysicalMemoryManager: {:?}",
                        e
                    );
                    Err(e)
                } else {
                    Self::allocate_physical_memory(size, align_order, pm_manager)
                }
            }
            Err(e) => {
                pr_err!("Failed to allocate physical memory: {:?}", e);
                Err(e)
            }
        }
    }

    pub fn create_user_memory_manager(&self) -> Result<Self, MemoryError> {
        assert!(self.is_kernel_memory_manager());
        let mut user_virtual_memory_manager = VirtualMemoryManager::new();

        user_virtual_memory_manager
            .init_user(&self.virtual_memory_manager, get_physical_memory_manager())?;

        return Ok(Self::new(user_virtual_memory_manager));
    }

    fn _alloc_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: MemoryOptionFlags,
    ) -> Result<(VAddress, PAddress), MemoryError> {
        /* Return physically continuous 2 ^ order pages memory. */
        let size = order.to_offset();
        let pm_manager = get_physical_memory_manager();
        let physical_address =
            Self::allocate_physical_memory(size, MOrder::new(PAGE_SHIFT), pm_manager)?;

        match self.virtual_memory_manager.alloc_and_map_virtual_address(
            size,
            physical_address,
            permission,
            option,
            pm_manager,
        ) {
            Ok(address) => {
                self._clone_kernel_memory_pages_if_needed()?;
                Ok((address, physical_address))
            }
            Err(e) => {
                if let Err(e) = pm_manager.free(physical_address, size, false) {
                    pr_err!("Failed to free physical memory: {:?}", e);
                }
                Err(e)
            }
        }
    }

    pub fn alloc_pages_with_physical_address(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<(VAddress, PAddress), MemoryError> {
        Self::check_option_and_permission(&permission, &option)?;
        self._alloc_pages(
            order,
            permission,
            option.unwrap_or(MemoryOptionFlags::KERNEL) | MemoryOptionFlags::ALLOC,
        )
    }

    pub fn alloc_pages(
        &mut self,
        order: MPageOrder,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        self.alloc_pages_with_physical_address(order, permission, option)
            .and_then(|r| Ok(r.0))
    }

    pub fn alloc_nonlinear_pages(
        &mut self,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        if size <= PAGE_SIZE {
            return self.alloc_pages(MPageOrder::new(0), permission, option);
        }
        let size = MSize::new((size.to_usize() - 1) & PAGE_MASK) + PAGE_SIZE;
        let vm_entry = self.virtual_memory_manager.alloc_virtual_address(
            size,
            permission,
            option.unwrap_or(MemoryOptionFlags::KERNEL | MemoryOptionFlags::ALLOC),
        )?;
        let vm_start_address = vm_entry.get_vm_start_address();
        let pm_manager = get_physical_memory_manager();

        for i in MIndex::new(0)..size.to_index() {
            match Self::allocate_physical_memory(PAGE_SIZE, MOrder::new(PAGE_SHIFT), pm_manager) {
                Ok(physical_address) => {
                    if let Err(e) = self
                        .virtual_memory_manager
                        .map_physical_address_into_vm_entry_and_page_table(
                            vm_entry,
                            vm_start_address + i.to_offset(),
                            physical_address,
                            PAGE_SIZE,
                            pm_manager,
                        )
                    {
                        pr_err!("Failed to map memory memory: {:?}", e);
                        if let Err(e) = pm_manager.free(physical_address, PAGE_SIZE, false) {
                            pr_err!("Failed to free physical memory: {:?}", e);
                        }
                        if let Err(e) = self
                            .virtual_memory_manager
                            .free_address_with_vm_entry(vm_entry, pm_manager)
                        {
                            pr_err!("Failed to free memory: {:?}", e);
                        }
                        return Err(MemoryError::AllocAddressFailed);
                    }
                }
                Err(e) => {
                    pr_err!("Failed to allocate physical memory: {:?}", e);
                    if let Err(e) = self
                        .virtual_memory_manager
                        .free_address_with_vm_entry(vm_entry, pm_manager)
                    {
                        pr_err!("Failed to free memory: {:?}", e);
                    }
                    return Err(MemoryError::AllocAddressFailed);
                }
            }
        }

        self._clone_kernel_memory_pages_if_needed()?;
        return Ok(vm_start_address);
    }

    pub fn free(&mut self, address: VAddress) -> Result<(), MemoryError> {
        let pm_manager = get_physical_memory_manager();
        let aligned_vm_address = address & PAGE_MASK;
        if let Err(e) = self
            .virtual_memory_manager
            .free_address(VAddress::new(aligned_vm_address), pm_manager)
        {
            pr_err!("Failed to free memory: {:?}", e); /* The error of 'free_address' tends to be ignored. */
            return Err(e);
        }
        self._clone_kernel_memory_pages_if_needed()?;
        return Ok(());
        /* Freeing Physical Memory will be done by Virtual Memory Manager, if it be needed. */
    }

    pub fn free_physical_memory(
        &mut self,
        address: PAddress,
        size: MSize,
    ) -> Result<(), MemoryError> {
        /* initializing use only */

        if let Err(e) = get_physical_memory_manager().free(address, size, false) {
            pr_err!("Failed to free physical memory: {:?}", e);
            Err(e)
        } else {
            Ok(())
        }
    }

    pub fn share_kernel_memory_with_user(
        &mut self,
        user_memory_manager: &mut MemoryManager,
        kernel_virtual_address: VAddress,
        user_virtual_address_to_map: VAddress,
        user_permission: MemoryPermissionFlags,
        user_option: MemoryOptionFlags,
    ) -> Result<(), MemoryError> {
        if !self.is_kernel_memory_manager() || user_memory_manager.is_kernel_memory_manager() {
            pr_err!("Invalid Operation.");
            return Err(MemoryError::InternalError);
        }
        Self::check_option_and_permission(&user_permission, &Some(user_option))?;
        self.virtual_memory_manager.share_memory_with_user(
            &mut user_memory_manager.virtual_memory_manager,
            kernel_virtual_address,
            user_virtual_address_to_map,
            user_permission,
            user_option,
            get_physical_memory_manager(),
        )
    }

    pub fn get_physical_address_list(
        &self,
        virtual_address: VAddress,
        offset: MIndex,
        number_of_pages: MIndex,
        list_buffer: &mut [PAddress],
    ) -> Result<usize, MemoryError> {
        self.virtual_memory_manager.get_physical_address_list(
            virtual_address,
            offset,
            number_of_pages,
            list_buffer,
        )
    }

    pub fn io_remap(
        &mut self,
        physical_address: PAddress,
        size: MSize,
        permission: MemoryPermissionFlags,
        option: Option<MemoryOptionFlags>,
    ) -> Result<VAddress, MemoryError> {
        let (aligned_physical_address, aligned_size) = Self::page_align(physical_address, size);

        let pm_manager = get_physical_memory_manager();
        /* TODO: check physical_address is not allocatble */
        let option = option.unwrap_or(MemoryOptionFlags::KERNEL)
            | MemoryOptionFlags::IO_MAP
            | MemoryOptionFlags::DEVICE_MEMORY
            | MemoryOptionFlags::DO_NOT_FREE_PHYSICAL_ADDRESS;
        let virtual_address = self.virtual_memory_manager.map_address(
            aligned_physical_address,
            None,
            aligned_size,
            permission,
            option,
            pm_manager,
        )?;

        self._clone_kernel_memory_pages_if_needed()?;

        Ok(virtual_address + (physical_address - aligned_physical_address))
    }

    pub fn mremap(
        &mut self,
        old_virtual_address: VAddress,
        _old_size: MSize,
        new_size: MSize,
    ) -> Result<VAddress, MemoryError> {
        let (aligned_virtual_address, aligned_new_size) =
            Self::page_align(old_virtual_address, new_size);

        let pm_manager = get_physical_memory_manager();

        //pm_manager.reserve_memory(aligned_physical_address, size, false);
        /* physical_address must be reserved. */

        let new_virtual_address = self.virtual_memory_manager.resize_memory_mapping(
            aligned_virtual_address,
            aligned_new_size,
            pm_manager,
        )?;

        self._clone_kernel_memory_pages_if_needed()?;

        Ok(new_virtual_address + (old_virtual_address - aligned_virtual_address))
    }

    #[inline]
    fn check_option_and_permission(
        p: &MemoryPermissionFlags,
        o: &Option<MemoryOptionFlags>,
    ) -> Result<(), MemoryError> {
        if o.as_ref()
            .and_then(|o| Some(o.is_for_user() && !p.is_user_accessible()))
            .unwrap_or(false)
        {
            pr_err!("User Memory must be accessible from user.");
            return Err(MemoryError::InternalError);
        }

        return Ok(());
    }

    pub fn set_paging_table(&mut self) {
        self.virtual_memory_manager.flush_paging();
    }

    pub fn free_all_allocated_memory(&mut self) -> Result<(), MemoryError> {
        assert!(!self.is_kernel_memory_manager());
        self.virtual_memory_manager
            .free_all_mapping(get_physical_memory_manager())
    }

    pub fn dump_memory_manager(&self) {
        kprintln!("----Physical Memory Entries Dump----");
        if let Err(_) = get_physical_memory_manager().dump_memory_entry() {
            kprintln!("Failed to dump Physical Memory Manager");
        }
        kprintln!("----Physical Memory Entries Dump End----");
        kprintln!("----Virtual Memory Entries Dump----");
        self.virtual_memory_manager.dump_memory_manager(None, None);
        kprintln!("----Virtual Memory Entries Dump End----");
    }

    #[inline] /* want to be const... */
    pub fn page_align<T: Address>(address: T, size: MSize) -> (T /*address*/, MSize /*size*/) {
        if size.is_zero() && (address.to_usize() & PAGE_MASK) == 0 {
            (address, MSize::new(0))
        } else {
            (
                (address.to_usize() & PAGE_MASK).into(),
                MSize::new(
                    (size.to_usize() + (address.to_usize() - (address.to_usize() & PAGE_MASK)) - 1)
                        & PAGE_MASK,
                ) + PAGE_SIZE,
            )
        }
    }

    #[inline]
    pub const fn size_align(size: MSize) -> MSize {
        if size.is_zero() {
            size
        } else {
            MSize::new((size.to_usize() - 1) & PAGE_MASK) + PAGE_SIZE
        }
    }
}

#[macro_export]
macro_rules! io_remap {
    ($address:expr, $len:expr, $permission:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .io_remap($address, $len, $permission, None)
    };
    ($address:expr, $len:expr, $permission:expr,$option:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .io_remap($address, $len, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! mremap {
    ($old_address:expr, $old_size:expr, $new_size:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .mremap($old_address, $old_size, $new_size)
    };
}

#[macro_export]
macro_rules! alloc_pages {
    ($order:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages(
                $order,
                $crate::kernel::memory_manager::data_type::MemoryPermissionFlags::data(),
                None,
            )
    };
    ($order:expr, $permission:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages($order, $permission, None)
    };
    ($order:expr, $permission:expr, $option:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages($order, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! alloc_pages_with_physical_address {
    ($order:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages_with_physical_address(
                $order,
                $crate::kernel::memory_manager::data_type::MemoryPermissionFlags::data(),
                None,
            )
    };
    ($order:expr, $permission:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages_with_physical_address($order, $permission, None)
    };
    ($order:expr, $permission:expr, $option:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_pages_with_physical_address($order, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! alloc_non_linear_pages {
    ($size:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_nonlinear_pages(
                $size,
                $crate::kernel::memory_manager::data_type::MemoryPermissionFlags::data(),
                None,
            )
    };
    ($size:expr, $permission:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_nonlinear_pages($size, $permission, None)
    };
    ($size:expr, $permission:expr, $option:expr) => {
        crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .alloc_nonlinear_pages($size, $permission, Some($option))
    };
}

#[macro_export]
macro_rules! free_pages {
    ($address:expr) => {
        $crate::kernel::manager_cluster::get_kernel_manager_cluster()
            .kernel_memory_manager
            .free($address)
    };
}

#[macro_export]
macro_rules! kmalloc {
    ($size:expr) => {
        $crate::kernel::manager_cluster::get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc($size)
    };

    ($t:ty, $initial_value:expr) => {
        $crate::kernel::manager_cluster::get_cpu_manager_cluster()
            .memory_allocator
            .kmalloc($crate::kernel::memory_manager::data_type::MSize::new(
                core::mem::size_of::<$t>(),
            ))
            .and_then(|addr| {
                let o = unsafe { &mut *(addr.to_usize() as *mut $t) };
                init_struct!(o, $initial_value);
                Ok(o)
            })
    };
}

#[macro_export]
macro_rules! kfree {
    ($address:expr, $size:expr) => {
        $crate::kernel::manager_cluster::get_cpu_manager_cluster()
            .memory_allocator
            .kfree($address, $size)
    };

    ($data:expr) => {
        $crate::kernel::manager_cluster::get_cpu_manager_cluster()
            .memory_allocator
            .kfree(
                $crate::kernel::memory_manager::data_type::VAddress::new(
                    $data as *const _ as usize,
                ),
                $crate::kernel::memory_manager::data_type::MSize::new(core::mem::size_of_val(
                    $data,
                )),
            )
    };
}
