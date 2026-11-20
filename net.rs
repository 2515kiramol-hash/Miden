#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

#[derive(Copy, Clone, PartialEq)]
pub enum NetBackend {
    None,
    IntelE1000,
    Realtek8168,
}

#[derive(Copy, Clone, PartialEq)]
pub enum LinkState {
    NoAdapter,
    CableUnplugged,
    ObtainingIp,
    Online,
}

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

#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct E1000RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct E1000TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(align(4096))]
struct E1000Rings {
    rx: [E1000RxDesc; 16],
    tx: [E1000TxDesc; 16],
}

static mut E1000_RINGS: E1000Rings = E1000Rings {
    rx: [E1000RxDesc { addr: 0, length: 0, checksum: 0, status: 0, errors: 0, special: 0 }; 16],
    tx: [E1000TxDesc { addr: 0, length: 0, cso: 0, cmd: 0, status: 0, css: 0, special: 0 }; 16],
};
static mut E1000_RX_BUFS: [[u8; 2048]; 16] = [[0; 2048]; 16];
static mut E1000_TX_BUFS: [[u8; 2048]; 16] = [[0; 2048]; 16];
static mut INTEL_MMIO: usize = 0;
static mut E1000_RX_IDX: usize = 0;
static mut E1000_TX_IDX: usize = 0;

pub static mut ACTIVE_NET: NetBackend = NetBackend::None;
pub static mut DEVICE_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
pub static mut CURRENT_IP: [u8; 4] = [0; 4];
pub static mut GATEWAY_IP: [u8; 4] = [0; 4];
static mut GATEWAY_MAC: [u8; 6] = [0xFF; 6];
pub static mut HAS_INTERNET: bool = false;

static mut NET_TICK: u32 = 0;

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
    pci_write32(bus, dev, func, 0x04, cmd | 0x07);

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

    rtl_write8(0x37, 0x10);
    for _ in 0..500_000 {
        if (rtl_read8(0x37) & 0x10) == 0 {
            break;
        }
        asm!("pause");
    }

    let mac_ptr = DEVICE_MAC.as_mut_ptr();
    for i in 0..6 {
        write_volatile(mac_ptr.add(i), rtl_read8(i));
    }

    rtl_write8(0x50, 0xC0);

    let rx_descs_ptr = RTL_RX_DESCS.as_mut_ptr();
    let tx_descs_ptr = RTL_TX_DESCS.as_mut_ptr();
    for i in 0..16 {
        let rx_buf = RTL_RX_BUFS.as_ptr().add(i) as u64;
        let eor = if i == 15 { 1 << 30 } else { 0 };
        write_volatile(
            rx_descs_ptr.add(i),
                       RtlDesc {
                           opts1: (1 << 31) | eor | 2040,
                       opts2: 0,
                       addr_lo: rx_buf as u32,
                       addr_hi: (rx_buf >> 32) as u32,
                       },
        );
        write_volatile(
            tx_descs_ptr.add(i),
                       RtlDesc { opts1: eor, opts2: 0, addr_lo: 0, addr_hi: 0 },
        );
    }
    sync_cache(rx_descs_ptr as *const u8, 256);
    sync_cache(tx_descs_ptr as *const u8, 256);

    let rx_ring_addr = rx_descs_ptr as u64;
    let tx_ring_addr = tx_descs_ptr as u64;
    rtl_write32(0xE4, rx_ring_addr as u32);
    rtl_write32(0xE8, (rx_ring_addr >> 32) as u32);
    rtl_write32(0x20, tx_ring_addr as u32);
    rtl_write32(0x24, (tx_ring_addr >> 32) as u32);

    rtl_write16(0xDA, 0x1FFF);
    rtl_write8(0xEC, 0x3B);
    rtl_write32(0x44, 0x0000_E70F);
    rtl_write32(0x40, 0x0300_0700);

    rtl_write8(0x37, 0x0C);
    rtl_write8(0x50, 0x00);

    RTL_RX_IDX = 0;
    RTL_TX_IDX = 0;
    true
}

unsafe fn rtl_send_packet(data: &[u8]) -> bool {
    let len = data.len().min(1514);
    let idx = RTL_TX_IDX;
    let desc = RTL_TX_DESCS.as_mut_ptr().add(idx);
    sync_cache(desc as *const u8, 16);

    if (read_volatile(&(*desc).opts1) & (1 << 31)) != 0 {
        return false;
    }

    let buf = RTL_TX_BUFS.as_mut_ptr().add(idx) as *mut u8;
    core::ptr::copy_nonoverlapping(data.as_ptr(), buf, len);
    sync_cache(buf, len);

    let eor = if idx == 15 { 1 << 30 } else { 0 };
    let buf_addr = buf as u64;
    write_volatile(&mut (*desc).addr_lo, buf_addr as u32);
    write_volatile(&mut (*desc).addr_hi, (buf_addr >> 32) as u32);
    write_volatile(&mut (*desc).opts2, 0);
    write_volatile(&mut (*desc).opts1, (1 << 31) | eor | (1 << 29) | (1 << 28) | (len as u32));
    sync_cache(desc as *const u8, 16);

    rtl_write8(0x38, 0x40);
    RTL_TX_IDX = (idx + 1) % 16;
    true
}

unsafe fn rtl_recv_packet(out_buf: &mut [u8]) -> Option<usize> {
    let idx = RTL_RX_IDX;
    let desc = RTL_RX_DESCS.as_mut_ptr().add(idx);
    sync_cache(desc as *const u8, 16);

    let opts1 = read_volatile(&(*desc).opts1);
    if (opts1 & (1 << 31)) != 0 {
        return None;
    }

    let pkt_len = ((opts1 & 0x3FFF) as usize).saturating_sub(4);
    let copy_len = pkt_len.min(out_buf.len());
    if copy_len > 0 {
        let src = RTL_RX_BUFS.as_ptr().add(idx) as *const u8;
        sync_cache(src, copy_len);
        core::ptr::copy_nonoverlapping(src, out_buf.as_mut_ptr(), copy_len);
    }

    let eor = if idx == 15 { 1 << 30 } else { 0 };
    write_volatile(&mut (*desc).opts2, 0);
    write_volatile(&mut (*desc).opts1, (1 << 31) | eor | 2040);
    sync_cache(desc as *const u8, 16);

    RTL_RX_IDX = (idx + 1) % 16;
    if copy_len > 0 { Some(copy_len) } else { None }
}

unsafe fn e1000_write(reg: usize, val: u32) {
    write_volatile((INTEL_MMIO + reg) as *mut u32, val);
}

unsafe fn e1000_read(reg: usize) -> u32 {
    read_volatile((INTEL_MMIO + reg) as *const u32)
}

unsafe fn init_intel_e1000(bus: u8, dev: u8, func: u8) -> bool {
    let cmd = pci_read32(bus, dev, func, 0x04);
    pci_write32(bus, dev, func, 0x04, cmd | 0x07);

    let bar0 = pci_read32(bus, dev, func, 0x10) as u64;
    let is_64 = ((bar0 >> 1) & 0x03) == 2;
    let bar1 = if is_64 { pci_read32(bus, dev, func, 0x14) as u64 } else { 0 };
    INTEL_MMIO = ((bar1 << 32) | (bar0 & !0xF)) as usize;
    if INTEL_MMIO == 0 {
        return false;
    }

    e1000_write(0x0000, 1 << 26);
    for _ in 0..200_000 {
        if (e1000_read(0x0000) & (1 << 26)) == 0 {
            break;
        }
        asm!("pause");
    }

    e1000_write(0x0000, (1 << 6) | (1 << 5));

    let ral = e1000_read(0x5400);
    let rah = e1000_read(0x5404);
    if ral != 0 || (rah & 0xFFFF) != 0 {
        let mac_ptr = DEVICE_MAC.as_mut_ptr();
        write_volatile(mac_ptr.add(0), (ral & 0xFF) as u8);
        write_volatile(mac_ptr.add(1), ((ral >> 8) & 0xFF) as u8);
        write_volatile(mac_ptr.add(2), ((ral >> 16) & 0xFF) as u8);
        write_volatile(mac_ptr.add(3), ((ral >> 24) & 0xFF) as u8);
        write_volatile(mac_ptr.add(4), (rah & 0xFF) as u8);
        write_volatile(mac_ptr.add(5), ((rah >> 8) & 0xFF) as u8);
    }

    let rx_ptr = E1000_RINGS.rx.as_mut_ptr();
    for i in 0..16 {
        let buf_addr = E1000_RX_BUFS.as_ptr().add(i) as u64;
        write_volatile(
            rx_ptr.add(i),
                       E1000RxDesc { addr: buf_addr, length: 0, checksum: 0, status: 0, errors: 0, special: 0 },
        );
    }
    let rx_base = rx_ptr as u64;
    e1000_write(0x2800, rx_base as u32);
    e1000_write(0x2804, (rx_base >> 32) as u32);
    e1000_write(0x2808, 16 * 16);
    e1000_write(0x2810, 0);
    e1000_write(0x2818, 15);
    e1000_write(0x0100, (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 15) | (1 << 26));
    let tx_ptr = E1000_RINGS.tx.as_mut_ptr();
    for i in 0..16 {
        let buf_addr = E1000_TX_BUFS.as_ptr().add(i) as u64;
        write_volatile(
            tx_ptr.add(i),
                       E1000TxDesc { addr: buf_addr, length: 0, cso: 0, cmd: 0, status: 1, css: 0, special: 0 },
        );
    }
    let tx_base = tx_ptr as u64;
    e1000_write(0x3800, tx_base as u32);
    e1000_write(0x3804, (tx_base >> 32) as u32);
    e1000_write(0x3808, 16 * 16);
    e1000_write(0x3810, 0);
    e1000_write(0x3818, 0);
    e1000_write(0x0400, (1 << 1) | (1 << 3) | (15 << 4) | (64 << 12));
    E1000_RX_IDX = 0;
    E1000_TX_IDX = 0;
    true
}
unsafe fn e1000_send_packet(data: &[u8]) -> bool {
    let len = data.len().min(1514);
    let idx = E1000_TX_IDX;
    let desc = E1000_RINGS.tx.as_mut_ptr().add(idx);
    if (read_volatile(&(*desc).status) & 1) == 0 {
        return false;
    }
    let buf = E1000_TX_BUFS.as_mut_ptr().add(idx) as *mut u8;
    core::ptr::copy_nonoverlapping(data.as_ptr(), buf, len);
    sync_cache(buf, len);
    write_volatile(&mut (*desc).length, len as u16);
    write_volatile(&mut (*desc).status, 0);
    write_volatile(&mut (*desc).cmd, 0x0B);
    E1000_TX_IDX = (idx + 1) % 16;
    e1000_write(0x3818, E1000_TX_IDX as u32);
    true
}
unsafe fn e1000_recv_packet(out_buf: &mut [u8]) -> Option<usize> {
    let idx = E1000_RX_IDX;
    let desc = E1000_RINGS.rx.as_mut_ptr().add(idx);
    let status = read_volatile(&(*desc).status);
    if (status & 1) == 0 {
        return None;
    }
    let pkt_len = read_volatile(&(*desc).length) as usize;
    let copy_len = pkt_len.min(out_buf.len());
    if copy_len > 0 {
        let src = E1000_RX_BUFS.as_ptr().add(idx) as *const u8;
        sync_cache(src, copy_len);
        core::ptr::copy_nonoverlapping(src, out_buf.as_mut_ptr(), copy_len);
    }
    write_volatile(&mut (*desc).status, 0);
    e1000_write(0x2818, idx as u32);
    E1000_RX_IDX = (idx + 1) % 16;
    if copy_len > 0 { Some(copy_len) } else { None }
}
pub unsafe fn init_network() -> NetBackend {
    if true { return NetBackend::None; }
    ACTIVE_NET = NetBackend::None;
    CURRENT_IP = [0; 4];
    GATEWAY_IP = [0; 4];
    GATEWAY_MAC = [0xFF; 6];
    HAS_INTERNET = false;
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let id = pci_read32(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF || id == 0 {
                    continue;
                }
                let class_rev = pci_read32(bus, dev, func, 0x08);
                if ((class_rev >> 24) as u8) != 0x02 {
                    continue;
                }
                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;
                if vendor == 0x10EC
                    && (device == 0x8168 || device == 0x8169 || device == 0x8136 || device == 0x8125 || device == 0x8139)
                    {
                        if init_realtek_8168(bus, dev, func) {
                            ACTIVE_NET = NetBackend::Realtek8168;
                            return ACTIVE_NET;
                        }
                    }
                    if vendor == 0x8086 {
                        if init_intel_e1000(bus, dev, func) {
                            ACTIVE_NET = NetBackend::IntelE1000;
                            return ACTIVE_NET;
                        }
                    }
            }
        }
    }
    NetBackend::None
}
pub unsafe fn is_link_up() -> bool {
    if true { return true; }
    match ACTIVE_NET {
        NetBackend::Realtek8168 => (rtl_read8(0x6C) & 0x02) != 0,
        NetBackend::IntelE1000 => INTEL_MMIO != 0 && (e1000_read(0x08) & 0x02) != 0,
        NetBackend::None => false,
    }
}
pub unsafe fn get_link_state() -> LinkState {
    if ACTIVE_NET == NetBackend::None {
        return LinkState::NoAdapter;
    }
    if !is_link_up() {
        return LinkState::CableUnplugged;
    }
    if CURRENT_IP == [0, 0, 0, 0] {
        LinkState::ObtainingIp
    } else {
        LinkState::Online
    }
}
pub unsafe fn get_adapter_name() -> &'static str {
    match ACTIVE_NET {
        NetBackend::Realtek8168 => "Realtek PCIe GbE (AMD/Intel)",
        NetBackend::IntelE1000 => "Intel PRO/1000 Gigabit",
        NetBackend::None => "Адаптер не найден",
    }
}
pub unsafe fn net_send_frame(frame: &[u8]) -> bool {
    match ACTIVE_NET {
        NetBackend::Realtek8168 => rtl_send_packet(frame),
        NetBackend::IntelE1000 => e1000_send_packet(frame),
        NetBackend::None => false,
    }
}
pub unsafe fn net_recv_frame(buf: &mut [u8]) -> Option<usize> {
    match ACTIVE_NET {
        NetBackend::Realtek8168 => rtl_recv_packet(buf),
        NetBackend::IntelE1000 => e1000_recv_packet(buf),
        NetBackend::None => None,
    }
}
fn ipv4_checksum(hdr: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < hdr.len() {
        let word = ((hdr[i] as u32) << 8) | (hdr[i + 1] as u32);
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < hdr.len() {
        sum = sum.wrapping_add((hdr[i] as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], tcp_seg: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    sum = sum.wrapping_add(((src_ip[0] as u32) << 8) | (src_ip[1] as u32));
    sum = sum.wrapping_add(((src_ip[2] as u32) << 8) | (src_ip[3] as u32));
    sum = sum.wrapping_add(((dst_ip[0] as u32) << 8) | (dst_ip[1] as u32));
    sum = sum.wrapping_add(((dst_ip[2] as u32) << 8) | (dst_ip[3] as u32));
    sum = sum.wrapping_add(6); // Protocol TCP = 6
    sum = sum.wrapping_add(tcp_seg.len() as u32);
    let mut i = 0;
    while i + 1 < tcp_seg.len() {
        let word = ((tcp_seg[i] as u32) << 8) | (tcp_seg[i + 1] as u32);
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < tcp_seg.len() {
        sum = sum.wrapping_add((tcp_seg[i] as u32) << 8);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}
unsafe fn send_dhcp_packet(is_request: bool, offered_ip: [u8; 4], server_ip: [u8; 4]) {
    let mut pkt = [0u8; 342];
    let p = pkt.as_mut_ptr();
    for i in 0..6 {
        write_volatile(p.add(i), 0xFF);
        write_volatile(p.add(6 + i), DEVICE_MAC[i]);
        write_volatile(p.add(70 + i), DEVICE_MAC[i]);
    }
    write_volatile(p.add(12), 0x08);
    write_volatile(p.add(13), 0x00);
    write_volatile(p.add(14), 0x45);
    write_volatile(p.add(16), (328 >> 8) as u8);
    write_volatile(p.add(17), (328 & 0xFF) as u8);
    write_volatile(p.add(22), 64);
    write_volatile(p.add(23), 17);
    for i in 0..4 {
        write_volatile(p.add(30 + i), 0xFF);
    }
    let ip_csum = ipv4_checksum(&pkt[14..34]);
    write_volatile(p.add(24), (ip_csum >> 8) as u8);
    write_volatile(p.add(25), (ip_csum & 0xFF) as u8);
    write_volatile(p.add(35), 68);
    write_volatile(p.add(37), 67);
    write_volatile(p.add(38), (308 >> 8) as u8);
    write_volatile(p.add(39), (308 & 0xFF) as u8);
    write_volatile(p.add(42), 1);
    write_volatile(p.add(43), 1);
    write_volatile(p.add(44), 6);
    write_volatile(p.add(46), 0x12);
    write_volatile(p.add(47), 0x34);
    write_volatile(p.add(48), 0x56);
    write_volatile(p.add(49), 0x78);
    write_volatile(p.add(52), 0x80);
    write_volatile(p.add(278), 99);
    write_volatile(p.add(279), 130);
    write_volatile(p.add(280), 83);
    write_volatile(p.add(281), 99);
    write_volatile(p.add(282), 53);
    write_volatile(p.add(283), 1);
    write_volatile(p.add(284), if is_request { 3 } else { 1 });
    if is_request {
        write_volatile(p.add(285), 50);
        write_volatile(p.add(286), 4);
        for i in 0..4 {
            write_volatile(p.add(287 + i), offered_ip[i]);
        }
        write_volatile(p.add(291), 54);
        write_volatile(p.add(292), 4);
        for i in 0..4 {
            write_volatile(p.add(293 + i), server_ip[i]);
        }
        write_volatile(p.add(297), 255);
    } else {
        write_volatile(p.add(285), 55);
        write_volatile(p.add(286), 2);
        write_volatile(p.add(287), 1);
        write_volatile(p.add(288), 3);
        write_volatile(p.add(289), 255);
    }
    net_send_frame(&pkt);
}
unsafe fn send_arp_request(target_ip: [u8; 4]) {
    let mut pkt = [0u8; 42];
    let p = pkt.as_mut_ptr();
    for i in 0..6 {
        write_volatile(p.add(i), 0xFF);
        write_volatile(p.add(6 + i), DEVICE_MAC[i]);
        write_volatile(p.add(22 + i), DEVICE_MAC[i]);
    }
    write_volatile(p.add(12), 0x08);
    write_volatile(p.add(13), 0x06);
    write_volatile(p.add(15), 1);
    write_volatile(p.add(16), 0x08);
    write_volatile(p.add(18), 6);
    write_volatile(p.add(19), 4);
    write_volatile(p.add(21), 1);
    for i in 0..4 {
        write_volatile(p.add(28 + i), CURRENT_IP[i]);
        write_volatile(p.add(38 + i), target_ip[i]);
    }
    net_send_frame(&pkt);
}
pub unsafe fn send_internet_ping() {
    if CURRENT_IP == [0, 0, 0, 0] {
        return;
    }
    if GATEWAY_MAC == [0xFF; 6] && GATEWAY_IP != [0, 0, 0, 0] {
        send_arp_request(GATEWAY_IP);
        return;
    }
    let mut pkt = [0u8; 42];
    let p = pkt.as_mut_ptr();
    for i in 0..6 {
        write_volatile(p.add(i), GATEWAY_MAC[i]);
        write_volatile(p.add(6 + i), DEVICE_MAC[i]);
    }
    write_volatile(p.add(12), 0x08);
    write_volatile(p.add(13), 0x00);
    write_volatile(p.add(14), 0x45);
    write_volatile(p.add(17), 28);
    write_volatile(p.add(22), 64);
    write_volatile(p.add(23), 1);
    for i in 0..4 {
        write_volatile(p.add(26 + i), CURRENT_IP[i]);
        write_volatile(p.add(30 + i), 8);
    }
    let ip_csum = ipv4_checksum(&pkt[14..34]);
    write_volatile(p.add(24), (ip_csum >> 8) as u8);
    write_volatile(p.add(25), (ip_csum & 0xFF) as u8);
    write_volatile(p.add(34), 8);
    write_volatile(p.add(39), 1);
    write_volatile(p.add(41), 1);
    let icmp_csum = ipv4_checksum(&pkt[34..42]);
    write_volatile(p.add(36), (icmp_csum >> 8) as u8);
    write_volatile(p.add(37), (icmp_csum & 0xFF) as u8);
    net_send_frame(&pkt);
}
pub unsafe fn poll_network_stack() {
    if true { return; }
    if ACTIVE_NET == NetBackend::None || !is_link_up() {
        CURRENT_IP = [0; 4];
        GATEWAY_IP = [0; 4];
        GATEWAY_MAC = [0xFF; 6];
        HAS_INTERNET = false;
        return;
    }
    NET_TICK = NET_TICK.wrapping_add(1);
    if NET_TICK % 120 == 1 {
        if CURRENT_IP == [0, 0, 0, 0] {
            send_dhcp_packet(false, [0; 4], [0; 4]);
        } else {
            send_internet_ping();
        }
    }
    let mut rx = [0u8; 1600];
    while let Some(len) = net_recv_frame(&mut rx) {
        if len < 42 {
            continue;
        }
        let p = rx.as_ptr();
        let eth_type = ((read_volatile(p.add(12)) as u16) << 8) | (read_volatile(p.add(13)) as u16);
        if eth_type == 0x0806 {
            let op = ((read_volatile(p.add(20)) as u16) << 8) | (read_volatile(p.add(21)) as u16);
            if op == 2 {
                let mut sender_ip = [0u8; 4];
                for i in 0..4 {
                    sender_ip[i] = read_volatile(p.add(28 + i));
                }
                if sender_ip == GATEWAY_IP {
                    for i in 0..6 {
                        GATEWAY_MAC[i] = read_volatile(p.add(22 + i));
                    }
                    send_internet_ping();
                }
            }
        }
        if eth_type == 0x0800 {
            let proto = read_volatile(p.add(23));
            if proto == 1 && read_volatile(p.add(34)) == 0 {
                if read_volatile(p.add(26)) == 8 && read_volatile(p.add(27)) == 8 {
                    HAS_INTERNET = true;
                }
            }
            if proto == 17 && len >= 285 {
                let src_port = ((read_volatile(p.add(34)) as u16) << 8) | (read_volatile(p.add(35)) as u16);
                let dst_port = ((read_volatile(p.add(36)) as u16) << 8) | (read_volatile(p.add(37)) as u16);
                if src_port == 67 && dst_port == 68 && read_volatile(p.add(42)) == 2 {
                    let yiaddr = [
                        read_volatile(p.add(58)),
                        read_volatile(p.add(59)),
                        read_volatile(p.add(60)),
                        read_volatile(p.add(61)),
                    ];
                    let mut msg_type = 0u8;
                    let mut srv_ip = [0u8; 4];
                    let mut gw_ip = [0u8; 4];
                    let mut opt = 282usize;
                    while opt + 2 < len {
                        let code = read_volatile(p.add(opt));
                        if code == 255 {
                            break;
                        }
                        if code == 0 {
                            opt += 1;
                            continue;
                        }
                        let olen = read_volatile(p.add(opt + 1)) as usize;
                        if opt + 2 + olen > len {
                            break;
                        }
                        if code == 53 && olen >= 1 {
                            msg_type = read_volatile(p.add(opt + 2));
                        } else if code == 54 && olen >= 4 {
                            for i in 0..4 {
                                srv_ip[i] = read_volatile(p.add(opt + 2 + i));
                            }
                        } else if code == 3 && olen >= 4 {
                            for i in 0..4 {
                                gw_ip[i] = read_volatile(p.add(opt + 2 + i));
                            }
                        }
                        opt += 2 + olen;
                    }
                    if msg_type == 2 && yiaddr != [0, 0, 0, 0] {
                        send_dhcp_packet(true, yiaddr, srv_ip);
                    } else if msg_type == 5 && yiaddr != [0, 0, 0, 0] {
                        CURRENT_IP = yiaddr;
                        if gw_ip != [0, 0, 0, 0] {
                            GATEWAY_IP = gw_ip;
                        }
                        send_arp_request(GATEWAY_IP);
                    }
                }
            }
        }
    }
}
// ============================================================================
// DNS-РЕЗОЛВЕР И HTTP-ЗАГРУЗЧИК СТРАНИЦ ДЛЯ БРАУЗЕРА
// ============================================================================
/// Переводит домен (например "example.com") в IPv4 через DNS-сервер 8.8.8.8
pub unsafe fn dns_resolve(domain: &str) -> Option<[u8; 4]> {
    if CURRENT_IP == [0, 0, 0, 0] {
        return None;
    }
    if GATEWAY_MAC == [0xFF; 6] {
        send_arp_request(GATEWAY_IP);
        for _ in 0..200_000 {
            poll_network_stack();
            if GATEWAY_MAC != [0xFF; 6] {
                break;
            }
            asm!("pause");
        }
        if GATEWAY_MAC == [0xFF; 6] {
            return None;
        }
    }
    let mut pkt = [0u8; 256];
    let p = pkt.as_mut_ptr();
    for i in 0..6 {
        write_volatile(p.add(i), GATEWAY_MAC[i]);
        write_volatile(p.add(6 + i), DEVICE_MAC[i]);
    }
    write_volatile(p.add(12), 0x08);
    write_volatile(p.add(13), 0x00);
    // Кодируем домен в DNS QNAME (после 12-байтного DNS-заголовка, смещение 42 + 12 = 54)
    let mut pos = 54usize;
    for part in domain.split('.') {
        let bytes = part.as_bytes();
        if bytes.is_empty() || pos + bytes.len() + 6 >= 250 {
            continue;
        }
        write_volatile(p.add(pos), bytes.len() as u8);
        pos += 1;
        for &b in bytes {
            write_volatile(p.add(pos), b);
            pos += 1;
        }
    }
    write_volatile(p.add(pos), 0); // Конец QNAME
    pos += 1;
    // QTYPE = A (1), QCLASS = IN (1)
    write_volatile(p.add(pos + 1), 1);
    write_volatile(p.add(pos + 3), 1);
    pos += 4;
    let udp_len = (pos - 34) as u16;
    let ip_len = udp_len + 20;
    // IPv4 Header
    write_volatile(p.add(14), 0x45);
    write_volatile(p.add(16), (ip_len >> 8) as u8);
    write_volatile(p.add(17), (ip_len & 0xFF) as u8);
    write_volatile(p.add(22), 64);
    write_volatile(p.add(23), 17); // UDP
    for i in 0..4 {
        write_volatile(p.add(26 + i), CURRENT_IP[i]);
        write_volatile(p.add(30 + i), 8); // 8.8.8.8
    }
    let ip_csum = ipv4_checksum(&pkt[14..34]);
    write_volatile(p.add(24), (ip_csum >> 8) as u8);
    write_volatile(p.add(25), (ip_csum & 0xFF) as u8);
    // UDP Header (Port 53123 -> 53)
    write_volatile(p.add(34), (53123u16 >> 8) as u8);
    write_volatile(p.add(35), (53123u16 & 0xFF) as u8);
    write_volatile(p.add(37), 53);
    write_volatile(p.add(38), (udp_len >> 8) as u8);
    write_volatile(p.add(39), (udp_len & 0xFF) as u8);
    // DNS Header (ID = 0xBEEF, Flags = 0x0100 Standard Query Recursion Desired, QDCOUNT = 1)
    write_volatile(p.add(42), 0xBE);
    write_volatile(p.add(43), 0xEF);
    write_volatile(p.add(44), 0x01);
    write_volatile(p.add(47), 1);
    net_send_frame(&pkt[..pos]);
    // Ждем DNS-ответ
    let mut rx = [0u8; 1600];
    for _ in 0..1_500_000 {
        if let Some(len) = net_recv_frame(&mut rx) {
            if len > 60 {
                let rp = rx.as_ptr();
                if read_volatile(rp.add(23)) == 17
                    && read_volatile(rp.add(35)) == 53
                    && read_volatile(rp.add(42)) == 0xBE
                    && read_volatile(rp.add(43)) == 0xEF
                    {
                        let ancount = ((read_volatile(rp.add(48)) as u16) << 8) | (read_volatile(rp.add(49)) as u16);
                        if ancount > 0 {
                            // В конце ответа на A-запрос последние 4 байта — IPv4 адрес
                            return Some([
                                read_volatile(rp.add(len - 4)),
                                        read_volatile(rp.add(len - 3)),
                                        read_volatile(rp.add(len - 2)),
                                        read_volatile(rp.add(len - 1)),
                            ]);
                        }
                    }
            }
        }
        asm!("pause");
    }
    None
}
unsafe fn send_tcp_segment(
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) {
    let total_len = 54 + payload.len();
    let mut pkt = [0u8; 1500];
    let p = pkt.as_mut_ptr();
    for i in 0..6 {
        write_volatile(p.add(i), GATEWAY_MAC[i]);
        write_volatile(p.add(6 + i), DEVICE_MAC[i]);
    }
    write_volatile(p.add(12), 0x08);
    write_volatile(p.add(13), 0x00);
    let ip_len = (40 + payload.len()) as u16;
    write_volatile(p.add(14), 0x45);
    write_volatile(p.add(16), (ip_len >> 8) as u8);
    write_volatile(p.add(17), (ip_len & 0xFF) as u8);
    write_volatile(p.add(22), 64);
    write_volatile(p.add(23), 6); // TCP
    for i in 0..4 {
        write_volatile(p.add(26 + i), CURRENT_IP[i]);
        write_volatile(p.add(30 + i), dst_ip[i]);
    }
    let ip_csum = ipv4_checksum(&pkt[14..34]);
    write_volatile(p.add(24), (ip_csum >> 8) as u8);
    write_volatile(p.add(25), (ip_csum & 0xFF) as u8);
    // TCP Header (20 байт, смещение 34..54)
    write_volatile(p.add(34), (src_port >> 8) as u8);
    write_volatile(p.add(35), (src_port & 0xFF) as u8);
    write_volatile(p.add(36), (dst_port >> 8) as u8);
    write_volatile(p.add(37), (dst_port & 0xFF) as u8);
    for i in 0..4 {
        write_volatile(p.add(38 + i), ((seq >> (24 - i * 8)) & 0xFF) as u8);
        write_volatile(p.add(42 + i), ((ack >> (24 - i * 8)) & 0xFF) as u8);
    }
    write_volatile(p.add(46), 0x50); // Data offset = 5 (20 bytes)
    write_volatile(p.add(47), flags);
    write_volatile(p.add(48), 0x40); // Window = 16384
    write_volatile(p.add(49), 0x00);
    for (i, &b) in payload.iter().enumerate() {
        write_volatile(p.add(54 + i), b);
    }
    let t_csum = tcp_checksum(CURRENT_IP, dst_ip, &pkt[34..total_len]);
    write_volatile(p.add(50), (t_csum >> 8) as u8);
    write_volatile(p.add(51), (t_csum & 0xFF) as u8);
    net_send_frame(&pkt[..total_len]);
}
/// Скачивает веб-страницу по HTTP (порт 80) и записывает чистый текст в out_text
pub unsafe fn http_get_page(host: &str, out_text: &mut [u8]) -> usize {
    let Some(dst_ip) = dns_resolve(host) else {
        let msg = b"Error: DNS resolve failed (No Internet or unknown host)";
        let n = msg.len().min(out_text.len());
        out_text[..n].copy_from_slice(&msg[..n]);
        return n;
    };
    let src_port: u16 = 49152 + ((NET_TICK & 0x3FFF) as u16);
    let mut seq: u32 = 0x1000_0001;
    let mut ack: u32 = 0;
    // 1. Отправляем TCP SYN (flags = 0x02)
    send_tcp_segment(dst_ip, src_port, 80, seq, 0, 0x02, &[]);
    let mut rx = [0u8; 1600];
    let mut connected = false;
    for _ in 0..1_500_000 {
        if let Some(len) = net_recv_frame(&mut rx) {
            if len >= 54 {
                let rp = rx.as_ptr();
                if read_volatile(rp.add(23)) == 6 {
                    let r_dst_port = ((read_volatile(rp.add(36)) as u16) << 8) | (read_volatile(rp.add(37)) as u16);
                    let flags = read_volatile(rp.add(47));
                    // SYN + ACK (0x12)
                    if r_dst_port == src_port && (flags & 0x12) == 0x12 {
                        let srv_seq = ((read_volatile(rp.add(38)) as u32) << 24)
                        | ((read_volatile(rp.add(39)) as u32) << 16)
                        | ((read_volatile(rp.add(40)) as u32) << 8)
                        | (read_volatile(rp.add(41)) as u32);
                        seq += 1;
                        ack = srv_seq.wrapping_add(1);
                        // Отправляем ACK (0x10)
                        send_tcp_segment(dst_ip, src_port, 80, seq, ack, 0x10, &[]);
                        connected = true;
                        break;
                    }
                }
            }
        }
        asm!("pause");
    }
    if !connected {
        let msg = b"Error: TCP connection timeout (port 80)";
        let n = msg.len().min(out_text.len());
        out_text[..n].copy_from_slice(&msg[..n]);
        return n;
    }
    // 2. Формируем и отправляем HTTP GET запрос
    let mut req = [0u8; 256];
    let mut rlen = 0usize;
    for &b in b"GET / HTTP/1.0\r\nHost: " {
        req[rlen] = b;
        rlen += 1;
    }
    for &b in host.as_bytes() {
        if rlen < 200 {
            req[rlen] = b;
            rlen += 1;
        }
    }
    for &b in b"\r\nUser-Agent: midenOS/1.0\r\nConnection: close\r\n\r\n" {
        req[rlen] = b;
        rlen += 1;
    }
    send_tcp_segment(dst_ip, src_port, 80, seq, ack, 0x18, &req[..rlen]); // PSH + ACK
    seq = seq.wrapping_add(rlen as u32);
    // 3. Принимаем данные страницы и очищаем HTML-теги
    let mut written = 0usize;
    let mut in_tag = false;
    let mut header_done = false;
    let mut last_chars = [0u8; 4];
    for _ in 0..2_500_000 {
        if let Some(len) = net_recv_frame(&mut rx) {
            if len >= 54 {
                let rp = rx.as_ptr();
                if read_volatile(rp.add(23)) == 6 {
                    let r_dst_port = ((read_volatile(rp.add(36)) as u16) << 8) | (read_volatile(rp.add(37)) as u16);
                    if r_dst_port == src_port {
                        let tcp_hdr_len = ((read_volatile(rp.add(46)) >> 4) as usize) * 4;
                        let data_off = 34 + tcp_hdr_len;
                        let flags = read_volatile(rp.add(47));
                        if len > data_off {
                            let payload_len = len - data_off;
                            ack = ack.wrapping_add(payload_len as u32);
                            send_tcp_segment(dst_ip, src_port, 80, seq, ack, 0x10, &[]);
                            for i in data_off..len {
                                let b = read_volatile(rp.add(i));
                                if !header_done {
                                    last_chars[0] = last_chars[1];
                                    last_chars[1] = last_chars[2];
                                    last_chars[2] = last_chars[3];
                                    last_chars[3] = b;
                                    if last_chars == [b'\r', b'\n', b'\r', b'\n'] {
                                        header_done = true;
                                    }
                                    continue;
                                }
                                if b == b'<' {
                                    in_tag = true;
                                    continue;
                                }
                                if b == b'>' {
                                    in_tag = false;
                                    continue;
                                }
                                if !in_tag && written < out_text.len() {
                                    if b == b'\r' {
                                        continue;
                                    }
                                    out_text[written] = b;
                                    written += 1;
                                }
                            }
                        }
                        // FIN (0x01) или RST (0x04)
                        if (flags & 0x05) != 0 {
                            if (flags & 0x01) != 0 {
                                ack = ack.wrapping_add(1);
                                send_tcp_segment(dst_ip, src_port, 80, seq, ack, 0x11, &[]);
                            }
                            break;
                        }
                    }
                }
            }
        }
        asm!("pause");
    }
    if written == 0 {
        let msg = b"Connected, but empty HTTP body (HTTPS sites require TLS)";
        let n = msg.len().min(out_text.len());
        out_text[..n].copy_from_slice(&msg[..n]);
        return n;
    }
    written
}
