//!
//! Intel(R) Ethernet Controller I210
//!

use crate::arch::target_arch::paging::PAGE_SIZE_USIZE;

use crate::kernel::drivers::pci::{
    msi::setup_msi_or_msi_x, ClassCode, PciDevice, PciDeviceDriver, PciManager,
};
use crate::kernel::manager_cluster::get_kernel_manager_cluster;
use crate::kernel::memory_manager::data_type::*;
use crate::kernel::network_manager::ethernet_device::{
    EthernetDeviceDescriptor, EthernetDeviceDriver, EthernetDeviceInfo, TxEntry,
};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;

use crate::{alloc_pages_with_physical_address, free_pages, io_remap, kmalloc};

use core::mem::MaybeUninit;

use alloc::collections::LinkedList;

pub struct I210Manager {
    base_address: VAddress,
    transfer_ring_buffer: VAddress,
    transfer_ring_lock: IrqSaveSpinLockFlag,
    transfer_ids: [u32; Self::NUM_OF_TX_DESC],
    transfer_tail: u32,
    transfer_head: u32,
    receive_ring_buffer: VAddress,
    receive_ring_lock: IrqSaveSpinLockFlag,
    receive_tail: u32,
    receive_buffer: VAddress,
}

static mut I210_LIST: LinkedList<(usize, *mut I210Manager)> = LinkedList::new();

impl PciDeviceDriver for I210Manager {
    const BASE_CLASS_CODE: u8 = 0x02;
    const SUB_CLASS_CODE: u8 = 0x00;

    fn setup_device(pci_dev: &PciDevice, _class_code: ClassCode) -> Result<(), ()> {
        let pci_manager = &get_kernel_manager_cluster().pci_manager;

        let vendor_id = pci_manager.read_vendor_id(pci_dev).unwrap_or(0);
        if vendor_id != Self::VENDOR_ID {
            return Err(());
        }
        /*let device_id = pci_manager.read_data(pci_dev, 0x02, 0x02).unwrap_or(0) as u16;
        let mut is_target_device = false;
        for e in Self::DEVICE_ID {
            if device_id == e {
                is_target_device = true;
                break;
            }
        }
        if !is_target_device {
            return Err(());
        }*/
        let mut command_status =
            pci_manager.read_data(pci_dev, PciManager::PCI_CONFIGURATION_COMMAND, 4)?;
        command_status &= !PciManager::COMMAND_INTERRUPT_DISABLE_BIT;
        pci_manager.write_data(
            pci_dev,
            PciManager::PCI_CONFIGURATION_COMMAND,
            command_status,
        )?;
        let mut base_address = pci_manager.read_base_address_register(pci_dev, 0)? as usize;
        if (base_address & (1 << 2)) != 0 {
            base_address |= (pci_manager.read_base_address_register(pci_dev, 1)? as usize) << 32;
        }
        base_address &= !((1 << 4) - 1);
        pr_debug!("Base Address: {:#X}", base_address);
        let controller_base_address = io_remap!(
            PAddress::new(base_address),
            MSize::new(Self::REGISTER_MAP_SIZE),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        );
        if let Err(e) = controller_base_address {
            pr_err!("Failed to map memory: {:?}", e);
            return Err(());
        }
        let controller_base_address = controller_base_address.unwrap();
        write_mmio(controller_base_address, Self::CTRL_OFFSET, Self::CTRL_RST);
        let mut i = 0;
        while i < Self::SPIN_TIMEOUT {
            if (read_mmio::<u32>(controller_base_address, Self::CTRL_OFFSET) & Self::CTRL_RST) == 0
            {
                break;
            }
            i += 1;
        }
        if i == Self::SPIN_TIMEOUT {
            pr_err!("Failed to reset device");
            return Err(());
        }
        write_mmio(
            controller_base_address,
            Self::CTRL_OFFSET,
            Self::CTRL_FD | Self::CTRL_SLU,
        );

        let receive_packet_size = read_mmio::<u32>(controller_base_address, Self::RXPBSIZE_OFFSET)
            & Self::RXPBSIZE_RXPBSIZE;
        let transfer_packet_size = read_mmio::<u32>(controller_base_address, Self::TXPBSIZE_OFFSET)
            & Self::TXPBSIZE_TXPB0SIZE;
        pr_debug!(
            "RX Packet Size: {:#X} KB, TX Packet Size: {:#X} KB",
            receive_packet_size,
            transfer_packet_size
        );

        /* Allocate ring buffers */
        let (tx_ring_buffer_virtual_address, tx_ring_buffer_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(Self::TX_DESC_SIZE * Self::NUM_OF_TX_DESC)
                .page_align_up()
                .to_order(None)
                .to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to allocate memory: {:?}", e);
                return Err(());
            }
        };
        let (rx_ring_buffer_virtual_address, rx_ring_buffer_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(Self::RX_DESC_SIZE * Self::NUM_OF_RX_DESC)
                .page_align_up()
                .to_order(None)
                .to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                let _ = free_pages!(tx_ring_buffer_virtual_address);
                pr_err!("Failed to allocate memory: {:?}", e);
                return Err(());
            }
        };

        let (receive_buffer, receive_buffer_physical_address) = match alloc_pages_with_physical_address!(
            MSize::new(2048 * Self::NUM_OF_RX_DESC)
                .page_align_up()
                .to_order(None)
                .to_page_order(),
            MemoryPermissionFlags::data(),
            MemoryOptionFlags::DEVICE_MEMORY
        ) {
            Ok(a) => a,
            Err(e) => {
                let _ = free_pages!(tx_ring_buffer_virtual_address);
                let _ = free_pages!(rx_ring_buffer_virtual_address);
                pr_err!("Failed to allocate memory: {:?}", e);
                return Err(());
            }
        };
        for i in 0..Self::NUM_OF_RX_DESC {
            unsafe {
                *((rx_ring_buffer_virtual_address.to_usize() + i * Self::RX_DESC_SIZE)
                    as *mut u64) = (receive_buffer_physical_address.to_usize() + i * 2048) as u64;
                *((rx_ring_buffer_virtual_address.to_usize()
                    + i * Self::RX_DESC_SIZE
                    + core::mem::size_of::<u64>()) as *mut u64) = 0;
            };
        }

        for i in 0..Self::NUM_OF_TX_DESC {
            unsafe {
                *((tx_ring_buffer_virtual_address.to_usize()
                    + i * Self::TX_DESC_SIZE
                    + core::mem::size_of::<u64>()) as *mut u64) = 0
            };
        }

        /* Setup receive registers */
        write_mmio(
            controller_base_address,
            Self::RDBAL_OFFSET,
            (rx_ring_buffer_physical_address.to_usize() & u32::MAX as usize) as u32,
        );
        write_mmio(
            controller_base_address,
            Self::RDBAH_OFFSET,
            (rx_ring_buffer_physical_address.to_usize() >> u32::BITS) as u32,
        );
        write_mmio(
            controller_base_address,
            Self::RDLEN_OFFSET,
            (Self::RX_DESC_SIZE * Self::NUM_OF_RX_DESC) as u32,
        );

        write_mmio(controller_base_address, Self::RDH_OFFSET, 0u32);
        write_mmio(
            controller_base_address,
            Self::RDT_OFFSET,
            (Self::NUM_OF_RX_DESC - 1) as u32,
        );
        write_mmio(
            controller_base_address,
            Self::RCTL_OFFSET,
            Self::RCTL_RXEN
                | Self::RCTL_BAM
                | Self::RCTL_BSIZE_2048
                | Self::RCTL_SBP
                | Self::RCTL_UPE
                | Self::RCTL_MPE
                | Self::RCTL_SECRC,
        );

        /* Setup transfer registers */
        write_mmio(
            controller_base_address,
            Self::TIPG_OFFSET,
            (0x08 << Self::TIPG_IPGT_OFFSET)
                | (0x04 << Self::TIPG_IPGR1_OFFSET)
                | (0x06 << Self::TIPG_IPGR_OFFSET),
        );
        write_mmio(
            controller_base_address,
            Self::TDBAL_OFFSET,
            (tx_ring_buffer_physical_address.to_usize() & u32::MAX as usize) as u32,
        );
        write_mmio(
            controller_base_address,
            Self::TDBAH_OFFSET,
            (tx_ring_buffer_physical_address.to_usize() >> u32::BITS) as u32,
        );
        write_mmio(
            controller_base_address,
            Self::TDLEN_OFFSET,
            (Self::TX_DESC_SIZE * Self::NUM_OF_TX_DESC) as u32,
        );
        write_mmio(controller_base_address, Self::TDH_OFFSET, 0u32);
        write_mmio(controller_base_address, Self::TDT_OFFSET, 0u32);
        write_mmio(
            controller_base_address,
            Self::TCTL_OFFSET,
            Self::TCTL_TXEN | Self::TCTL_PSP | Self::TCTL_BAM,
        );

        /* get mac address */
        let mut mac_address: [u8; 6] = [0; 6];
        let i = u32::from_le(read_mmio::<u32>(
            controller_base_address,
            Self::INVM_DATA_OFFSET,
        ));
        mac_address[0] = (i & u8::MAX as u32) as u8;
        mac_address[1] = ((i >> u8::BITS) & u8::MAX as u32) as u8;
        mac_address[2] = ((i >> (u8::BITS * 2)) & u8::MAX as u32) as u8;
        mac_address[3] = (i >> (u8::BITS * 3)) as u8;
        let i = u32::from_le(read_mmio::<u32>(
            controller_base_address,
            Self::INVM_DATA_OFFSET + core::mem::size_of::<u32>(),
        ));
        mac_address[4] = (i & u8::MAX as u32) as u8;
        mac_address[5] = ((i >> u8::BITS) & u8::MAX as u32) as u8;
        if mac_address[0] == 0 {
            let controller = read_mmio::<u32>(controller_base_address, Self::EEC_OFFSET);
            if (controller & Self::EEC_EE_DET) != 0 {
                write_mmio(
                    controller_base_address,
                    Self::EEC_OFFSET,
                    controller | Self::EEC_EE_REQ,
                );
                let read_data = |address: u32| -> u16 {
                    write_mmio(
                        controller_base_address,
                        Self::EERD_OFFSET,
                        1 | (address << 2),
                    );
                    let mut i = 0;
                    let mut data: u32 = 0;
                    while i < Self::SPIN_TIMEOUT {
                        data = read_mmio(controller_base_address, Self::EERD_OFFSET);
                        if (data & (1 << 1)) != 0 {
                            break;
                        }
                        i += 1;
                    }
                    (data >> 16) as u16
                };
                for i in 0..=2 {
                    let d = read_data(i);
                    mac_address[2 * (i as usize)] = (d & u8::MAX as u16) as u8;
                    mac_address[2 * (i as usize) + 1] = (d >> u8::BITS) as u8;
                }
                write_mmio(controller_base_address, Self::EEC_OFFSET, controller);
            } else {
                pr_err!("EEPROM is not accessible");
            }
        }
        if mac_address[0] == 0 {
            let d = read_mmio::<u32>(controller_base_address, Self::RAL_OFFSET);
            mac_address[0] = (d & u8::MAX as u32) as u8;
            mac_address[1] = ((d >> u8::BITS) & u8::MAX as u32) as u8;
            mac_address[2] = ((d >> (u8::BITS * 2)) & u8::MAX as u32) as u8;
            mac_address[3] = (d >> (u8::BITS * 3)) as u8;
            let d = read_mmio::<u32>(
                controller_base_address,
                Self::RAL_OFFSET + core::mem::size_of::<u32>(),
            );
            mac_address[4] = (d & u8::MAX as u32) as u8;
            mac_address[5] = ((d >> u8::BITS) & u8::MAX as u32) as u8;
        }
        if mac_address[0] == 0 {
            pr_err!("Failed to get MAC Address");
        }

        let manager = match kmalloc!(
            Self,
            Self {
                base_address: controller_base_address,
                transfer_ring_buffer: tx_ring_buffer_virtual_address,
                transfer_ids: [0; Self::NUM_OF_TX_DESC],
                transfer_tail: 0,
                transfer_head: 0,
                transfer_ring_lock: IrqSaveSpinLockFlag::new(),
                receive_ring_buffer: rx_ring_buffer_virtual_address,
                receive_tail: 0,
                receive_ring_lock: IrqSaveSpinLockFlag::new(),
                receive_buffer,
            }
        ) {
            Ok(e) => e,
            Err(e) => {
                pr_err!("Failed to initialize manager: {:?}", e);
                return Err(());
            }
        };

        if let Ok(interrupt_id) = setup_msi_or_msi_x(pci_dev, i210_handler, None, false) {
            unsafe { I210_LIST.push_back((interrupt_id, manager as *mut _)) };
            write_mmio(controller_base_address, Self::IMC_OFFSET, u32::MAX);
            /* Clear Interrupt  Status */
            let _ = read_mmio::<u32>(controller_base_address, Self::ICR_OFFSET);
            /* Enable interrupt */
            write_mmio(
                controller_base_address,
                Self::IMS_OFFSET,
                (1u32 << 7) | (1u32 << 0) | (1u32 << 1/* For compatibility */),
            );
        }

        let descriptor = EthernetDeviceDescriptor::new(mac_address, manager);
        get_kernel_manager_cluster()
            .ethernet_device_manager
            .add_device(descriptor);
        return Ok(());
    }
}

impl EthernetDeviceDriver for I210Manager {
    fn send(&mut self, _info: &EthernetDeviceInfo, entry: &mut TxEntry) -> Result<MSize, ()> {
        if entry.get_length().to_usize() > u16::MAX as usize {
            return Err(());
        }
        let mut physical_address_list: [PAddress; (u16::MAX as usize) / PAGE_SIZE_USIZE] =
            unsafe { MaybeUninit::assume_init(MaybeUninit::uninit()) };
        let result = get_kernel_manager_cluster()
            .kernel_memory_manager
            .get_physical_address_list(
                entry.get_buffer(),
                MIndex::new(0),
                entry.get_length().page_align_up().to_index(),
                &mut physical_address_list,
            );
        if let Err(e) = result {
            pr_err!("Failed to get physical address list: {:?}", e);
            return Err(());
        }
        let pages = result.unwrap();
        let mut processed_pages = 0;
        let mut processed_size = 0;
        while processed_pages < pages && processed_size < entry.get_length().to_usize() {
            let mut i = 1;
            let mut size_to_send = 0;
            loop {
                if (size_to_send + PAGE_SIZE_USIZE)
                    < (entry.get_length().to_usize() - processed_size)
                {
                    if physical_address_list[processed_pages + i]
                        != physical_address_list[processed_pages] + MIndex::new(i).to_offset()
                    {
                        break;
                    }
                    size_to_send += PAGE_SIZE_USIZE;
                    i += 1;
                } else {
                    size_to_send += entry.get_length().to_usize() - processed_size - size_to_send;
                    break;
                }
            }
            let n = self.transfer_data_legacy(
                physical_address_list[processed_pages],
                MSize::new(size_to_send),
                entry.get_id(),
            )?;
            entry.set_number_of_frame(n + entry.get_number_of_frame());
            processed_pages += i;
            processed_size += size_to_send;
        }

        return Ok(MSize::new(processed_size));
    }
}

fn read_mmio<T: Sized>(base: VAddress, offset: usize) -> T {
    unsafe { core::ptr::read_volatile((base.to_usize() + offset) as *const T) }
}

fn write_mmio<T: Sized>(base: VAddress, offset: usize, data: T) {
    unsafe { core::ptr::write_volatile((base.to_usize() + offset) as *mut T, data) }
}

impl I210Manager {
    const VENDOR_ID: u16 = 0x8086;
    //const DEVICE_ID: [u16; 5] = [0x1531, 0x1533, 0x1536, 0x1537, 0x1538];

    const REGISTER_MAP_SIZE: usize = 128 * 1024;

    const TX_DESC_SIZE: usize = 128;
    const NUM_OF_TX_DESC: usize = 32;
    const RX_DESC_SIZE: usize = 128;
    const NUM_OF_RX_DESC: usize = 32;

    const CTRL_OFFSET: usize = 0x0000;
    const CTRL_FD: u32 = 1;
    const CTRL_SLU: u32 = 1 << 6;
    const CTRL_RST: u32 = 1 << 26;

    //const ICR_OFFSET: usize = 0x1500;
    const ICR_OFFSET: usize = 0x00C0;
    const ICR_TX_FINISHED: u32 = 0b11;
    const ICR_RX_FINISHED: u32 = 1 << 7;

    const RCTL_OFFSET: usize = 0x0100;
    const RCTL_RXEN: u32 = 1 << 1;
    const RCTL_SBP: u32 = 1 << 2;
    const RCTL_UPE: u32 = 1 << 3;
    const RCTL_MPE: u32 = 1 << 4;
    const RCTL_BAM: u32 = 1 << 15;
    const RCTL_BSIZE_2048: u32 = 0b00 << 16;
    const RCTL_SECRC: u32 = 1 << 26;

    const TCTL_OFFSET: usize = 0x0400;
    const TCTL_TXEN: u32 = 1 << 1;
    const TCTL_PSP: u32 = 1 << 3;
    const TCTL_BAM: u32 = 1 << 15;

    const TIPG_OFFSET: usize = 0x0410;
    const TIPG_IPGT_OFFSET: u32 = 0x00;
    const TIPG_IPGR1_OFFSET: u32 = 0x0A;
    const TIPG_IPGR_OFFSET: u32 = 0x14;

    //const IMS_OFFSET: usize = 0x1508;
    const IMS_OFFSET: usize = 0x00D0;
    //const IMC_OFFSET: usize = 0x150C;
    const IMC_OFFSET: usize = 0x00D8;

    const RXPBSIZE_OFFSET: usize = 0x2404;
    const RXPBSIZE_RXPBSIZE: u32 = (1 << 6) - 1;

    const RDBAL_OFFSET: usize = 0x2800;
    const RDBAH_OFFSET: usize = 0x2804;
    const RDLEN_OFFSET: usize = 0x2808;

    const RDH_OFFSET: usize = 0x02810;
    const RDT_OFFSET: usize = 0x02818;

    const TXPBSIZE_OFFSET: usize = 0x3404;
    const TXPBSIZE_TXPB0SIZE: u32 = (1 << 6) - 1;

    const TDBAL_OFFSET: usize = 0x3800;
    const TDBAH_OFFSET: usize = 0x3804;
    const TDLEN_OFFSET: usize = 0x3808;

    const TDH_OFFSET: usize = 0x03810;
    const TDT_OFFSET: usize = 0x03818;

    //const RAL_OFFSET: usize = 0x5400;
    const RAL_OFFSET: usize = 0x0040;

    //const EEC_OFFSET:usize = 0x12010;
    //const EERD_OFFSET:usize = 0x12014;
    const EEC_OFFSET: usize = 0x0010;
    const EERD_OFFSET: usize = 0x0014;
    const EEC_EE_REQ: u32 = 1 << 6;
    const EEC_EE_DET: u32 = 1 << 19;

    const INVM_DATA_OFFSET: usize = 0x12120;

    const SPIN_TIMEOUT: u32 = 0x10000;

    fn transfer_data_legacy(
        &mut self,
        buffer: PAddress,
        length: MSize,
        id: u32,
    ) -> Result<usize, ()> {
        const CMD_EOP: u64 = 1 << 24;
        const CMD_RS: u64 = 1 << 27;
        let mut remaining_length = length.to_usize();
        let mut number_of_descriptors = 0;
        let _lock = self.transfer_ring_lock.lock();

        while remaining_length > 0 {
            let transfer_length = if remaining_length > u16::MAX as usize {
                u16::MAX as usize
            } else {
                remaining_length
            };

            let mut command = transfer_length as u64;
            if transfer_length == remaining_length {
                command |= CMD_EOP;
            }
            command |= CMD_RS;
            let descriptor: [u64; 2] = [buffer.to_usize() as u64, command];

            unsafe {
                *((self.transfer_ring_buffer.to_usize()
                    + ((self.transfer_tail as usize) * (2 * core::mem::size_of::<u64>())))
                    as *mut u64) = descriptor[0];
                *((self.transfer_ring_buffer.to_usize()
                    + ((self.transfer_tail as usize) * (2 * core::mem::size_of::<u64>())
                        + core::mem::size_of::<u64>())) as *mut u64) = descriptor[1];
            }
            self.transfer_ids[self.transfer_tail as usize] = id;
            self.transfer_tail += 1;
            if self.transfer_tail == Self::NUM_OF_TX_DESC as u32 {
                self.transfer_tail = 0;
            }
            write_mmio(self.base_address, Self::TDT_OFFSET, self.transfer_tail);
            remaining_length -= transfer_length;
            number_of_descriptors += 1;
        }
        drop(_lock);
        return Ok(number_of_descriptors);
    }

    pub fn interrupt_handler(&mut self) {
        let icr = read_mmio::<u32>(self.base_address, Self::ICR_OFFSET);
        pr_debug!("ICR: {:#X}", icr);

        if (icr & Self::ICR_RX_FINISHED) != 0 {
            pr_debug!("Received Packet");
            let _lock = self.receive_ring_lock.lock();
            let receive_ring_buffer = unsafe {
                &mut *(self.receive_ring_buffer.to_usize()
                    as *mut [u64; Self::NUM_OF_RX_DESC * Self::RX_DESC_SIZE
                        / core::mem::size_of::<u64>()])
            };
            let mut cursor = self.receive_tail as usize;
            while ((receive_ring_buffer[2 * cursor + 1] >> 31) & 0x01) != 0 {
                let length = receive_ring_buffer[2 * cursor + 1] & (1 << 16 - 1);
                pr_debug!("{cursor}: {length}");
                if length > 0 {
                    let buffer = kmalloc!(MSize::new(length as usize));
                    if let Ok(buffer) = buffer {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                (self.receive_buffer.to_usize() + 2048 * cursor) as *const u8,
                                buffer.to_usize() as *mut u8,
                                length as usize,
                            )
                        };
                        /* Throw ethernet manager */
                    } else {
                        pr_err!("Failed to allocate memory: {:?}", buffer.unwrap_err());
                    }
                }
                receive_ring_buffer[2 * cursor + 1] = 0;
                write_mmio(self.base_address, Self::RDT_OFFSET, cursor as u32);
                cursor = (cursor + 1) & (Self::NUM_OF_RX_DESC - 1);
            }
        }
        if (icr & Self::ICR_TX_FINISHED) != 0 {
            let _lock = self.transfer_ring_lock.lock();
            let transfer_ring_buffer = unsafe {
                &mut *(self.transfer_ring_buffer.to_usize()
                    as *mut [u64; Self::NUM_OF_TX_DESC * Self::TX_DESC_SIZE
                        / core::mem::size_of::<u64>()])
            };

            while ((transfer_ring_buffer[2 * (self.transfer_head as usize) + 1] >> 24) & (1 << 3)/* CMD::RS */)
                != 0
            {
                //let cmd = ((receive_ring_buffer[2 * self.transfer_head + 1] >> 24) & 0xff) as u8;
                let id = self.transfer_ids[self.transfer_head as usize];
                let done = transfer_ring_buffer[2 * (self.transfer_head as usize) + 1] & (1 << 32);
                if done == 0 {
                    pr_err!("Failed to transmit frame: id:{id}");
                }
                get_kernel_manager_cluster()
                    .ethernet_device_manager
                    .update_transmit_status(id, done != 0);
                transfer_ring_buffer[2 * (self.transfer_head as usize) + 1] = 0;
                self.transfer_head += 1;
                if self.transfer_head == Self::NUM_OF_TX_DESC as u32 {
                    self.transfer_head = 0;
                }
            }
        }
    }
}

fn i210_handler(index: usize) -> bool {
    if let Some(i210) = unsafe {
        I210_LIST
            .iter()
            .find(|x| (**x).0 == index)
            .and_then(|x| Some(x.1.clone()))
    } {
        unsafe { &mut *(i210) }.interrupt_handler();
        true
    } else {
        pr_err!("Unknown i210 Device");
        false
    }
}
