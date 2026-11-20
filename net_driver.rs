#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

#[derive(Copy, Clone, PartialEq)]
pub enum NetBackend {
    None,
    IntelE1000,
    Realtek8168,
    UsbCdcEcm,
}

#[repr(align(4096))]
struct NetDmaPage([u8; 4096]);

// Дескрипторы и буферы для Realtek RTL8111/8168 (16 RX + 16 TX)
#[repr(C, align(256))]
#[derive(Copy, Clone)]
struct RtlDesc {
    opts1: u32,
    opts2: u32,
    addr_lo: u32,
    addr_hi: u32,
}

static mut RTL_RX_DESCS: [RtlDesc; 16] = [RtlDesc { opts1: 0, opts2: 0, addr_lo: 0, addr_hi: 0 }; 16];
static mut RTL_TX_DESCS: [RtlDesc; 16] = [RtlDesc { opts1: 0, opts2: 0, addr_lo: 0, addr_hi: 0 }; 16];
static mut RTL_RX_BUFS: [[u8; 2048]; 16] = [[0; 2048]; 16];
static mut RTL_TX_BUFS: [[u8; 2048]; 16] = [[0; 2048]; 16];

static mut RTL_MMIO: usize = 0;
static mut RTL_IO_BASE: u16 = 0;
static mut RTL_RX_IDX: usize = 0;
static mut RTL_TX_IDX: usize = 0;

// Буферы для USB CDC-ECM / RNDIS (телефон / USB-адаптер через xHCI)
static mut USB_NET_EP_IN_RING: NetDmaPage = NetDmaPage([0; 4096]);
static mut USB_NET_EP_OUT_RING: NetDmaPage = NetDmaPage([0; 4096]);
static mut USB_NET_RX_BUF: [u8; 2048] = [0; 2048];
static mut USB_NET_TX_BUF: [u8; 2048] = [0; 2048];
static mut USB_NET_SLOT: u8 = 0;
static mut USB_NET_IN_DCI: u32 = 0;
static mut USB_NET_OUT_DCI: u32 = 0;
static mut USB_NET_IS_RNDIS: bool = false;

pub static mut ACTIVE_NET: NetBackend = NetBackend::None;
pub static mut DEVICE_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

#[inline(always)]
unsafe fn sync_cache(ptr: *const u8, len: usize) {
    asm!("mfence", options(nostack, preserves_flags));
    let mut off = 0;
    while off < len {
        let p = ptr.add(off);
        asm!("clflush [{}]", in(reg) p, options(nostack, preserves_flags));
        off += 64;
    }
    asm!("mfence", options(nostack, preserves_flags));
}

unsafe fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr: u32 = 0x8000_0000
    | ((bus as u32) << 16)
    | ((dev as u32) << 11)
    | ((func as u32) << 8)
    | ((offset as u32) & 0xFC);
    asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") addr, options(nomem, nostack));
    let val: u32;
    asm!("in eax, dx", out("eax") val, in("dx") 0xCFCu16, options(nomem, nostack));
    val
}

unsafe fn pci_write32(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    let addr: u32 = 0x8000_0000
    | ((bus as u32) << 16)
    | ((dev as u32) << 11)
    | ((func as u32) << 8)
    | ((offset as u32) & 0xFC);
    asm!("out dx, eax", in("dx") 0xCF8u16, in("eax") addr, options(nomem, nostack));
    asm!("out dx, eax", in("dx") 0xCFCu16, in("eax") val, options(nomem, nostack));
}

// ============================================================================
// 1. ДРАЙВЕР REALTEK RTL8111 / RTL8168 (Материнские платы AMD и Intel)
// ============================================================================

unsafe fn rtl_write8(reg: usize, val: u8) {
    if RTL_MMIO != 0 {
        write_volatile((RTL_MMIO + reg) as *mut u8, val);
    } else if RTL_IO_BASE != 0 {
        asm!("out dx, al", in("dx") RTL_IO_BASE + (reg as u16), in("al") val, options(nomem, nostack));
    }
}

unsafe fn rtl_read8(reg: usize) -> u8 {
    if RTL_MMIO != 0 {
        read_volatile((RTL_MMIO + reg) as *const u8)
    } else if RTL_IO_BASE != 0 {
        let v: u8;
        asm!("in al, dx", out("al") v, in("dx") RTL_IO_BASE + (reg as u16), options(nomem, nostack));
        v
    } else {
        0
    }
}

unsafe fn rtl_write16(reg: usize, val: u16) {
    if RTL_MMIO != 0 {
        write_volatile((RTL_MMIO + reg) as *mut u16, val);
    } else if RTL_IO_BASE != 0 {
        asm!("out dx, ax", in("dx") RTL_IO_BASE + (reg as u16), in("ax") val, options(nomem, nostack));
    }
}

unsafe fn rtl_write32(reg: usize, val: u32) {
    if RTL_MMIO != 0 {
        write_volatile((RTL_MMIO + reg) as *mut u32, val);
    } else if RTL_IO_BASE != 0 {
        asm!("out dx, eax", in("dx") RTL_IO_BASE + (reg as u16), in("eax") val, options(nomem, nostack));
    }
}

unsafe fn init_realtek_8168(bus: u8, dev: u8, func: u8) -> bool {
    let cmd = pci_read32(bus, dev, func, 0x04);
    pci_write32(bus, dev, func, 0x04, cmd | 0x07); // I/O + Memory + Bus Master

    let bar0 = pci_read32(bus, dev, func, 0x10);
    let bar2 = pci_read32(bus, dev, func, 0x18) as u64;
    let bar3 = pci_read32(bus, dev, func, 0x1C) as u64;

    if (bar0 & 1) != 0 {
        RTL_IO_BASE = (bar0 & !0x3) as u16;
    }
    let mmio = (bar3 << 32) | (bar2 & !0xF);
    if mmio != 0 {
        RTL_MMIO = mmio as usize;
    }
    if RTL_MMIO == 0 && RTL_IO_BASE == 0 {
        return false;
    }

    // Сброс контроллера Realtek (Command Register 0x37, bit 4 = Reset)
    rtl_write8(0x37, 0x10);
    for _ in 0..500_000 {
        if (rtl_read8(0x37) & 0x10) == 0 {
            break;
        }
        asm!("pause");
    }

    // Чтение аппаратного MAC-адреса (регистры 0x00..0x05)
    for i in 0..6 {
        DEVICE_MAC[i] = rtl_read8(i);
    }

    // Разблокировка регистров конфигурации (9346CR = 0xC0)
    rtl_write8(0x50, 0xC0);

    // Инициализация колец дескрипторов RX и TX
    for i in 0..16 {
        let rx_buf = RTL_RX_BUFS[i].as_ptr() as u64;
        let eor = if i == 15 { 1 << 30 } else { 0 };
        RTL_RX_DESCS[i] = RtlDesc {
            opts1: (1 << 31) | eor | 2040, // OWN bit = 1 (принадлежит карте)
            opts2: 0,
            addr_lo: rx_buf as u32,
            addr_hi: (rx_buf >> 32) as u32,
        };
        RTL_TX_DESCS[i] = RtlDesc {
            opts1: eor,
            opts2: 0,
            addr_lo: 0,
            addr_hi: 0,
        };
    }
    sync_cache(RTL_RX_DESCS.as_ptr() as *const u8, 256);
    sync_cache(RTL_TX_DESCS.as_ptr() as *const u8, 256);

    // Передача адресов колец дескрипторов в карту
    let rx_ring_addr = RTL_RX_DESCS.as_ptr() as u64;
    let tx_ring_addr = RTL_TX_DESCS.as_ptr() as u64;
    rtl_write32(0xE4, rx_ring_addr as u32);
    rtl_write32(0xE8, (rx_ring_addr >> 32) as u32);
    rtl_write32(0x20, tx_ring_addr as u32);
    rtl_write32(0x24, (tx_ring_addr >> 32) as u32);

    // Максимальный размер пакета и конфигурация RX/TX
    rtl_write16(0xDA, 0x1FFF); // RMS (Max RX packet size)
    rtl_write8(0xEC, 0x3B);  // Max TX packet size
    rtl_write32(0x44, 0x0000_E70F); // RCR: принимать Broadcast, Multicast, Physical Match
    rtl_write32(0x40, 0x0300_0700); // TCR: стандартный Interframe Gap + DMA burst

    // Включаем приёмник и передатчик (RE | TE = 0x0C)
    rtl_write8(0x37, 0x0C);
    rtl_write8(0x50, 0x00); // Блокируем конфигурацию

    RTL_RX_IDX = 0;
    RTL_TX_IDX = 0;
    true
}

unsafe fn rtl_send_packet(data: &[u8]) -> bool {
    let len = data.len().min(1514);
    let idx = RTL_TX_IDX;
    let desc = &mut RTL_TX_DESCS[idx];
    sync_cache(desc as *const RtlDesc as *const u8, 16);

    if (desc.opts1 & (1 << 31)) != 0 {
        return false; // Дескриптор занят картой
    }

    let buf = RTL_TX_BUFS[idx].as_mut_ptr();
    core::ptr::copy_nonoverlapping(data.as_ptr(), buf, len);
    sync_cache(buf, len);

    let eor = if idx == 15 { 1 << 30 } else { 0 };
    let buf_addr = buf as u64;
    desc.addr_lo = buf_addr as u32;
    desc.addr_hi = (buf_addr >> 32) as u32;
    desc.opts2 = 0;
    // OWN(31) | FS(29) | LS(28) | len
    desc.opts1 = (1 << 31) | eor | (1 << 29) | (1 << 28) | (len as u32);
    sync_cache(desc as *const RtlDesc as *const u8, 16);

    // Оповещаем карту (TPPoll: NPQ = bit 6)
    rtl_write8(0x38, 0x40);
    RTL_TX_IDX = (idx + 1) % 16;
    true
}

unsafe fn rtl_recv_packet(out_buf: &mut [u8]) -> Option<usize> {
    let idx = RTL_RX_IDX;
    let desc = &mut RTL_RX_DESCS[idx];
    sync_cache(desc as *const RtlDesc as *const u8, 16);

    // Если OWN == 1, пакет ещё не записан картой
    if (desc.opts1 & (1 << 31)) != 0 {
        return None;
    }

    let pkt_len = ((desc.opts1 & 0x3FFF) as usize).saturating_sub(4); // минус 4 байта CRC
    let copy_len = pkt_len.min(out_buf.len());
    if copy_len > 0 {
        let src = RTL_RX_BUFS[idx].as_ptr();
        sync_cache(src, copy_len);
        core::ptr::copy_nonoverlapping(src, out_buf.as_mut_ptr(), copy_len);
    }

    // Возвращаем дескриптор сетевой карте
    let eor = if idx == 15 { 1 << 30 } else { 0 };
    desc.opts2 = 0;
    desc.opts1 = (1 << 31) | eor | 2040;
    sync_cache(desc as *const RtlDesc as *const u8, 16);

    RTL_RX_IDX = (idx + 1) % 16;
    if copy_len > 0 { Some(copy_len) } else { None }
}

// ============================================================================
// 2. ЕДИНЫЙ ИНТЕРФЕЙС АВТОДЕТЕКТА И ОБМЕНА ПАКЕТАМИ ДЛЯ MIDENOS
// ============================================================================

pub unsafe fn init_network() -> NetBackend {
    // 1. Сканируем шину PCI на наличие сетевых карт (Class 0x02)
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let id = pci_read32(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF || id == 0 {
                    continue;
                }
                let class_rev = pci_read32(bus, dev, func, 0x08);
                let class = (class_rev >> 24) as u8;
                if class != 0x02 {
                    continue;
                }

                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;

                // Realtek (материнские платы AMD / Intel: RTL8111, RTL8168, RTL8125)
                if vendor == 0x10EC && (device == 0x8168 || device == 0x8169 || device == 0x8136 || device == 0x8125) {
                    if init_realtek_8168(bus, dev, func) {
                        ACTIVE_NET = NetBackend::Realtek8168;
                        return ACTIVE_NET;
                    }
                }

                // Intel (QEMU e1000 / e1000e / I217 / I219)
                if vendor == 0x8086 {
                    let cmd = pci_read32(bus, dev, func, 0x04);
                    pci_write32(bus, dev, func, 0x04, cmd | 0x07);
                    ACTIVE_NET = NetBackend::IntelE1000;
                    return ACTIVE_NET;
                }
            }
        }
    }
    // 2. Если PCI-карта не найдена — проверяем USB-модем телефона / USB-адаптер через xHCI
    if USB_NET_SLOT != 0 {
        ACTIVE_NET = NetBackend::UsbCdcEcm;
        return ACTIVE_NET;
    }
    NetBackend::None
}
/// Отправка сырого Ethernet-фрейма через активный драйвер
pub unsafe fn net_send_frame(frame: &[u8]) -> bool {
    match ACTIVE_NET {
        NetBackend::Realtek8168 => rtl_send_packet(frame),
        NetBackend::IntelE1000 => {
            // Вызов вашего крейта e1000-driver:
            // e1000_device.e1000_transmit(frame);
            true
        }
        NetBackend::UsbCdcEcm => {
            // Если это RNDIS (Android USB-модем), добавляем 44-байтный заголовок RNDIS_PACKET_MSG
            let mut pkt_len = frame.len();
            if USB_NET_IS_RNDIS {
                let hdr = USB_NET_TX_BUF.as_mut_ptr() as *mut u32;
                write_volatile(hdr.add(0), 0x0000_0001); // REMOTE_NDIS_PACKET_MSG
                write_volatile(hdr.add(1), (44 + frame.len()) as u32);
                write_volatile(hdr.add(2), 36); // DataOffset
                write_volatile(hdr.add(3), frame.len() as u32);
                core::ptr::copy_nonoverlapping(frame.as_ptr(), USB_NET_TX_BUF.as_mut_ptr().add(44), frame.len().min(1514));
                pkt_len += 44;
            } else {
                core::ptr::copy_nonoverlapping(frame.as_ptr(), USB_NET_TX_BUF.as_mut_ptr(), frame.len().min(1514));
            }
            pkt_len > 0
        }
        NetBackend::None => false,
    }
}
/// Чтение входящего Ethernet-фрейма из активного драйвера
pub unsafe fn net_recv_frame(buf: &mut [u8]) -> Option {
    match ACTIVE_NET {
        NetBackend::Realtek8168 => rtl_recv_packet(buf),
        NetBackend::IntelE1000 => {
            // Вызов вашего крейта e1000-driver:
            // if let Some(pkt) = e1000_device.e1000_recv() { ... }
            None
        }
        NetBackend::UsbCdcEcm => None,
        NetBackend::None => None,
    }
}
