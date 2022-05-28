//!
//! Ethernet RX/TX Device Manager
//!
//!

use super::{arp, ipv4};

use crate::kernel::manager_cluster::{get_cpu_manager_cluster, get_kernel_manager_cluster};
use crate::kernel::memory_manager::data_type::{Address, MSize, VAddress};
use crate::kernel::sync::spin_lock::IrqSaveSpinLockFlag;
use crate::kernel::task_manager::work_queue::WorkList;
use crate::kernel::task_manager::ThreadEntry;

use crate::{alloc_non_linear_pages, free_pages, kfree, kmalloc};

use alloc::collections::LinkedList;
use alloc::vec::Vec;

/*const PREAMBLE: (u8, usize) = (0b10101010, 7);
const SFD: u8 = 0b10101011;
const IPG: usize = 12;
const MIN_FRAME_DATA_SIZE: usize = 46;*/
const ETHERNET_PAYLOAD_OFFSET: usize = 14;
const MAX_FRAME_DATA_SIZE: usize = 1500;
const MAX_FRAME_SIZE: usize = MAX_FRAME_DATA_SIZE + 30 /*+ IPG*/ /*+ 8*/;
const MAC_ADDRESS_SIZE: usize = 6;

pub trait EthernetDeviceDriver {
    fn send(&mut self, info: &EthernetDeviceInfo, entry: &mut TxEntry) -> Result<MSize, ()>;
}

#[derive(Clone)]
pub struct EthernetDeviceInfo {
    pub mac_address: [u8; 6],
}

#[derive(Clone)]
pub struct EthernetDeviceDescriptor {
    info: EthernetDeviceInfo,
    driver: *mut dyn EthernetDeviceDriver,
}

pub struct EthernetDeviceManager {
    lock: IrqSaveSpinLockFlag,
    device_list: Vec<EthernetDeviceDescriptor>,
    tx_list: LinkedList<TxEntry>,
    next_id: u32,
}

pub struct TxEntry {
    entry_id: u32,
    buffer: VAddress,
    length: MSize,
    number_of_remaining_frames: usize,
    thread: Option<&'static mut ThreadEntry>,
    result: u8,
}

impl TxEntry {
    pub fn get_buffer(&self) -> VAddress {
        self.buffer
    }

    pub fn get_id(&self) -> u32 {
        self.entry_id
    }

    pub fn get_length(&self) -> MSize {
        self.length
    }

    pub fn get_number_of_frame(&self) -> usize {
        self.number_of_remaining_frames
    }

    pub fn set_number_of_frame(&mut self, n: usize) {
        self.number_of_remaining_frames = n;
    }
}

#[allow(dead_code)]
pub struct RxEntry {
    buffer: VAddress,
    length: MSize,
    number_of_remaining_frames: usize,
    thread: Option<&'static mut ThreadEntry>,
    result: u8,
}

impl EthernetDeviceManager {
    pub const fn new() -> Self {
        Self {
            lock: IrqSaveSpinLockFlag::new(),
            device_list: Vec::new(),
            tx_list: LinkedList::new(),
            next_id: 0,
        }
    }

    pub fn add_device(&mut self, d: EthernetDeviceDescriptor) {
        let _lock = self.lock.lock();
        self.device_list.push(d);
        drop(_lock);
    }

    pub fn send_data(
        &mut self,
        device_id: usize,
        data: &[u8],
        target_mac_address: [u8; 6],
        ether_type: u16,
    ) -> Result<(), ()> {
        if device_id >= self.device_list.len() {
            return Err(());
        }
        let needed_buffer_size = MSize::new(
            data.len()
                + ((data.len() / MAX_FRAME_DATA_SIZE).max(1)
                    * (MAX_FRAME_SIZE - MAX_FRAME_DATA_SIZE)),
        )
        .page_align_up();
        pr_debug!("Needed Buffer size for frames: {needed_buffer_size}");
        let send_buffer = match alloc_non_linear_pages!(needed_buffer_size) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to allocate address: {:?}", e);
                return Err(());
            }
        };
        let _lock = self.lock.lock();
        let descriptor = &self.device_list[device_id];
        let result = create_ethernet_frame(
            descriptor,
            send_buffer,
            needed_buffer_size,
            target_mac_address,
            ether_type,
            VAddress::new(data.as_ptr() as usize),
            MSize::new(data.len()),
        );
        if let Err(e) = result {
            pr_err!("Failed to create packet: {:?}", e);
            let _ = free_pages!(send_buffer);
            return Err(());
        }
        let mut entry = TxEntry {
            entry_id: self.next_id,
            buffer: send_buffer,
            length: result.unwrap(),
            number_of_remaining_frames: 0,
            thread: None,
            result: 0,
        };
        let result = unsafe { &mut *(self.device_list[device_id].driver) }
            .send(&self.device_list[device_id].info, &mut entry);
        if result.is_ok() {
            self.next_id = self.next_id.overflowing_add(1).0;
            self.tx_list.push_back(entry);
        } else {
            let _ = free_pages!(send_buffer);
        }
        drop(_lock);
        return result.and_then(|_| Ok(()));
    }

    pub fn get_mac_address(&self, device_id: usize) -> Result<[u8; 6], ()> {
        if device_id >= self.device_list.len() {
            return Err(());
        }
        Ok(self.device_list[device_id].info.mac_address)
    }

    pub fn update_transmit_status(&mut self, id: u32, is_successful: bool) {
        let _lock = self.lock.lock();
        if self.tx_list.len() == 0 {
            return;
        }
        let mut cursor = self.tx_list.cursor_front_mut();
        loop {
            let e = cursor.current().unwrap();
            if e.entry_id == id {
                /* Found */
                if !is_successful {
                    e.result |= 1;
                }
                e.number_of_remaining_frames -= 1;
                if e.number_of_remaining_frames == 0 {
                    let thread = core::mem::replace(&mut e.thread, None);
                    if let Some(thread) = thread {
                        if let Err(error) = get_kernel_manager_cluster()
                            .task_manager
                            .wake_up_thread(thread)
                        {
                            pr_err!("Failed to wake up the thread: {:?}", error);
                        }
                    } else {
                        //pr_debug!("Free the buffer({})", e.buffer);
                        let _ = free_pages!(e.buffer);
                        let _ = cursor.remove_current();
                    }
                }
                break;
            }
            if cursor.peek_next().is_none() {
                break;
            }
            cursor.move_next();
        }
    }

    const WORKER_DATA_SIZE: usize = core::mem::size_of::<(VAddress, MSize)>();

    pub fn received_data_handler(&mut self, allocated_data: VAddress, length: MSize) {
        let address = match kmalloc!(MSize::new(Self::WORKER_DATA_SIZE)) {
            Ok(a) => a,
            Err(e) => {
                pr_err!("Failed to allocate memory: {:?}", e);
                let _ = kfree!(allocated_data, length);
                return;
            }
        };
        unsafe { *(address.to_usize() as *mut (VAddress, MSize)) = (allocated_data, length) };

        let work = WorkList::new(Self::packet_worker, address.to_usize());
        if let Err(e) = get_cpu_manager_cluster().work_queue.add_work(work) {
            pr_err!("Failed to add worker: {:?}", e);
            let _ = kfree!(allocated_data, length);
            let _ = kfree!(address, MSize::new(Self::WORKER_DATA_SIZE));
        }
    }

    pub fn packet_worker(data: usize) {
        let allocated_data_address: VAddress;
        let data_length: MSize;
        unsafe { (allocated_data_address, data_length) = *(data as *const (VAddress, MSize)) };
        let _ = kfree!(VAddress::new(data), MSize::new(Self::WORKER_DATA_SIZE));

        let sender_mac_address =
            unsafe { *((allocated_data_address.to_usize() + 6) as *const [u8; 6]) };
        let frame_type = u16::from_be_bytes(unsafe {
            *((allocated_data_address.to_usize() + 12) as *const [u8; 2])
        });

        match frame_type {
            ipv4::ETHERNET_TYPE_IPV4 => {
                ipv4::ipv4_packet_handler(
                    allocated_data_address,
                    data_length,
                    ETHERNET_PAYLOAD_OFFSET,
                    sender_mac_address,
                );
            }
            arp::ETHERNET_TYPE_ARP => {
                arp::arp_packet_handler(
                    allocated_data_address,
                    data_length,
                    ETHERNET_PAYLOAD_OFFSET,
                    sender_mac_address,
                );
            }
            t => {
                pr_err!("Unknown frame_type: {:#X}", t);
                pr_debug!("Data: {:#X?}", unsafe {
                    core::slice::from_raw_parts(
                        allocated_data_address.to_usize() as *const u8,
                        data_length.to_usize(),
                    )
                });
                let _ = kfree!(allocated_data_address, data_length);
            }
        }
    }
}

impl EthernetDeviceDescriptor {
    pub fn new(mac_address: [u8; 6], driver: *mut dyn EthernetDeviceDriver) -> Self {
        Self {
            info: EthernetDeviceInfo { mac_address },
            driver,
        }
    }
}

fn create_ethernet_frame(
    ethernet_descriptor: &EthernetDeviceDescriptor,
    send_buffer: VAddress,
    send_buffer_size: MSize,
    target_mac_address: [u8; 6],
    frame_type: u16,
    data_buffer: VAddress,
    data_size: MSize,
) -> Result<MSize, ()> {
    let max_number_of_frames = send_buffer_size.to_usize() / MAX_FRAME_SIZE;
    let mut sent_size: usize = 0;
    let mut buffer_pointer: usize = 0;

    for _ in 0..max_number_of_frames {
        //let crc_start: usize;
        unsafe {
            /*core::ptr::write_bytes(
                (send_buffer.to_usize() + buffer_pointer) as *mut u8,
                PREAMBLE.0,
                PREAMBLE.1,
            );
            buffer_pointer += PREAMBLE.1;
            *((send_buffer.to_usize() + buffer_pointer) as *mut u8) = SFD;
            buffer_pointer += core::mem::size_of_val(&SFD);
            crc_start = send_buffer.to_usize() + buffer_pointer;*/
            *((send_buffer.to_usize() + buffer_pointer) as *mut [u8; 6]) = target_mac_address;
            buffer_pointer += MAC_ADDRESS_SIZE;
            *((send_buffer.to_usize() + buffer_pointer) as *mut [u8; 6]) =
                ethernet_descriptor.info.mac_address;
            buffer_pointer += MAC_ADDRESS_SIZE;
            *((send_buffer.to_usize() + buffer_pointer) as *mut [u8; 2]) = frame_type.to_be_bytes();
            buffer_pointer += core::mem::size_of_val(&frame_type);
        };
        let size_to_send = (data_size.to_usize() - sent_size).min(MAX_FRAME_DATA_SIZE);
        unsafe {
            core::ptr::copy_nonoverlapping(
                (data_buffer.to_usize() + sent_size) as *const u8,
                (send_buffer.to_usize() + buffer_pointer) as *mut u8,
                size_to_send,
            )
        };
        buffer_pointer += size_to_send;
        /*if size_to_send < MIN_FRAME_DATA_SIZE {
            unsafe {
                core::ptr::write_bytes(
                    (send_buffer.to_usize() + buffer_pointer) as *mut u8,
                    0,
                    MIN_FRAME_DATA_SIZE - size_to_send,
                )
            };
            buffer_pointer += MIN_FRAME_DATA_SIZE - size_to_send;
        }*/

        /* Calculate CRC32 */
        /*let mut crc_u32 = u32::MAX;
        for i in crc_start..(send_buffer.to_usize() + buffer_pointer) {
            crc_u32 = CRC_TABLE
                [((crc_u32 ^ (unsafe { *(i as *const u8) } as u32)) & (u8::MAX as u32)) as usize]
                ^ (crc_u32 >> 8);
        }
        crc_u32 ^= u32::MAX;
        unsafe { *((send_buffer.to_usize() + buffer_pointer) as *mut u32) = crc_u32.to_be() };
        buffer_pointer += core::mem::size_of_val(&crc_u32);
        unsafe {
            core::ptr::write_bytes((send_buffer.to_usize() + buffer_pointer) as *mut u8, 0, IPG)
        };
        buffer_pointer += IPG;
        */
        sent_size += size_to_send;
        if sent_size >= data_size.to_usize() {
            break;
        }
    }
    return Ok(MSize::new(buffer_pointer));
}

/*
const CRC_TABLE: [u32; 256] = [
    0x0, 0x77073096, 0xEE0E612C, 0x990951BA, 0x76DC419, 0x706AF48F, 0xE963A535, 0x9E6495A3,
    0xEDB8832, 0x79DCB8A4, 0xE0D5E91E, 0x97D2D988, 0x9B64C2B, 0x7EB17CBD, 0xE7B82D07, 0x90BF1D91,
    0x1DB71064, 0x6AB020F2, 0xF3B97148, 0x84BE41DE, 0x1ADAD47D, 0x6DDDE4EB, 0xF4D4B551, 0x83D385C7,
    0x136C9856, 0x646BA8C0, 0xFD62F97A, 0x8A65C9EC, 0x14015C4F, 0x63066CD9, 0xFA0F3D63, 0x8D080DF5,
    0x3B6E20C8, 0x4C69105E, 0xD56041E4, 0xA2677172, 0x3C03E4D1, 0x4B04D447, 0xD20D85FD, 0xA50AB56B,
    0x35B5A8FA, 0x42B2986C, 0xDBBBC9D6, 0xACBCF940, 0x32D86CE3, 0x45DF5C75, 0xDCD60DCF, 0xABD13D59,
    0x26D930AC, 0x51DE003A, 0xC8D75180, 0xBFD06116, 0x21B4F4B5, 0x56B3C423, 0xCFBA9599, 0xB8BDA50F,
    0x2802B89E, 0x5F058808, 0xC60CD9B2, 0xB10BE924, 0x2F6F7C87, 0x58684C11, 0xC1611DAB, 0xB6662D3D,
    0x76DC4190, 0x1DB7106, 0x98D220BC, 0xEFD5102A, 0x71B18589, 0x6B6B51F, 0x9FBFE4A5, 0xE8B8D433,
    0x7807C9A2, 0xF00F934, 0x9609A88E, 0xE10E9818, 0x7F6A0DBB, 0x86D3D2D, 0x91646C97, 0xE6635C01,
    0x6B6B51F4, 0x1C6C6162, 0x856530D8, 0xF262004E, 0x6C0695ED, 0x1B01A57B, 0x8208F4C1, 0xF50FC457,
    0x65B0D9C6, 0x12B7E950, 0x8BBEB8EA, 0xFCB9887C, 0x62DD1DDF, 0x15DA2D49, 0x8CD37CF3, 0xFBD44C65,
    0x4DB26158, 0x3AB551CE, 0xA3BC0074, 0xD4BB30E2, 0x4ADFA541, 0x3DD895D7, 0xA4D1C46D, 0xD3D6F4FB,
    0x4369E96A, 0x346ED9FC, 0xAD678846, 0xDA60B8D0, 0x44042D73, 0x33031DE5, 0xAA0A4C5F, 0xDD0D7CC9,
    0x5005713C, 0x270241AA, 0xBE0B1010, 0xC90C2086, 0x5768B525, 0x206F85B3, 0xB966D409, 0xCE61E49F,
    0x5EDEF90E, 0x29D9C998, 0xB0D09822, 0xC7D7A8B4, 0x59B33D17, 0x2EB40D81, 0xB7BD5C3B, 0xC0BA6CAD,
    0xEDB88320, 0x9ABFB3B6, 0x3B6E20C, 0x74B1D29A, 0xEAD54739, 0x9DD277AF, 0x4DB2615, 0x73DC1683,
    0xE3630B12, 0x94643B84, 0xD6D6A3E, 0x7A6A5AA8, 0xE40ECF0B, 0x9309FF9D, 0xA00AE27, 0x7D079EB1,
    0xF00F9344, 0x8708A3D2, 0x1E01F268, 0x6906C2FE, 0xF762575D, 0x806567CB, 0x196C3671, 0x6E6B06E7,
    0xFED41B76, 0x89D32BE0, 0x10DA7A5A, 0x67DD4ACC, 0xF9B9DF6F, 0x8EBEEFF9, 0x17B7BE43, 0x60B08ED5,
    0xD6D6A3E8, 0xA1D1937E, 0x38D8C2C4, 0x4FDFF252, 0xD1BB67F1, 0xA6BC5767, 0x3FB506DD, 0x48B2364B,
    0xD80D2BDA, 0xAF0A1B4C, 0x36034AF6, 0x41047A60, 0xDF60EFC3, 0xA867DF55, 0x316E8EEF, 0x4669BE79,
    0xCB61B38C, 0xBC66831A, 0x256FD2A0, 0x5268E236, 0xCC0C7795, 0xBB0B4703, 0x220216B9, 0x5505262F,
    0xC5BA3BBE, 0xB2BD0B28, 0x2BB45A92, 0x5CB36A04, 0xC2D7FFA7, 0xB5D0CF31, 0x2CD99E8B, 0x5BDEAE1D,
    0x9B64C2B0, 0xEC63F226, 0x756AA39C, 0x26D930A, 0x9C0906A9, 0xEB0E363F, 0x72076785, 0x5005713,
    0x95BF4A82, 0xE2B87A14, 0x7BB12BAE, 0xCB61B38, 0x92D28E9B, 0xE5D5BE0D, 0x7CDCEFB7, 0xBDBDF21,
    0x86D3D2D4, 0xF1D4E242, 0x68DDB3F8, 0x1FDA836E, 0x81BE16CD, 0xF6B9265B, 0x6FB077E1, 0x18B74777,
    0x88085AE6, 0xFF0F6A70, 0x66063BCA, 0x11010B5C, 0x8F659EFF, 0xF862AE69, 0x616BFFD3, 0x166CCF45,
    0xA00AE278, 0xD70DD2EE, 0x4E048354, 0x3903B3C2, 0xA7672661, 0xD06016F7, 0x4969474D, 0x3E6E77DB,
    0xAED16A4A, 0xD9D65ADC, 0x40DF0B66, 0x37D83BF0, 0xA9BCAE53, 0xDEBB9EC5, 0x47B2CF7F, 0x30B5FFE9,
    0xBDBDF21C, 0xCABAC28A, 0x53B39330, 0x24B4A3A6, 0xBAD03605, 0xCDD70693, 0x54DE5729, 0x23D967BF,
    0xB3667A2E, 0xC4614AB8, 0x5D681B02, 0x2A6F2B94, 0xB40BBE37, 0xC30C8EA1, 0x5A05DF1B, 0x2D02EF8D,
];*/
/*
/// CRC_TABLE was generated by this function
fn create_crc_table() -> [u32; 256] {
    let mut crc: [u32; 256] = [0; 256];
    for i in 0..crc.len() {
        let mut p = i as u32;
        for _ in 0..8 {
            p = if (p & 1) != 0 {
                0xEDB88320 ^ (p >> 1)
            } else {
                p >> 1
            };
        }
        crc[i] = p;
    }
    crc
}

fn print_crc_table() {
    println!("const CRC_TABLE: [u32; 256] = {:#X?}", create_crc_table());
}
*/
