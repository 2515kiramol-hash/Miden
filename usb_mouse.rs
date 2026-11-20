#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

#[derive(Copy, Clone, Default)]
pub struct UsbMouseEvent {
    pub dx: i8,
    pub dy: i8,
    pub buttons: u8,
}

#[repr(align(4096))]
struct DmaPage([u8; 4096]);

static mut DCBAA: DmaPage = DmaPage([0; 4096]);
static mut CMD_RING: DmaPage = DmaPage([0; 4096]);
static mut EVENT_RING: DmaPage = DmaPage([0; 4096]);
static mut ERST: DmaPage = DmaPage([0; 4096]);
static mut INPUT_CTX: DmaPage = DmaPage([0; 4096]);
static mut OUTPUT_CTX: DmaPage = DmaPage([0; 4096]);
static mut EP0_RING: DmaPage = DmaPage([0; 4096]);
static mut EP_INT_RING: DmaPage = DmaPage([0; 4096]);

#[repr(align(64))]
struct DmaBuf([u8; 256]);
static mut DESC_BUF: DmaBuf = DmaBuf([0; 256]);
static mut HID_BUF: DmaBuf = DmaBuf([0; 256]);

static mut XHCI_MMIO: usize = 0;
static mut XHCI_DB_OFF: usize = 0;
static mut XHCI_RTS_OFF: usize = 0;
static mut CTX_SIZE_64: bool = false;

static mut CMD_ENQ_IDX: usize = 0;
static mut CMD_CYCLE: u32 = 1;

static mut EVT_DEQ_IDX: usize = 0;
static mut EVT_CYCLE: u32 = 1;

static mut EP0_ENQ_IDX: usize = 0;
static mut EP0_CYCLE: u32 = 1;

static mut EP_INT_ENQ_IDX: usize = 0;
static mut EP_INT_CYCLE: u32 = 1;

static mut ACTIVE_SLOT: u8 = 0;
static mut MOUSE_DCI: u32 = 3;
static mut MOUSE_MPS: u32 = 8;
static mut USB_MOUSE_READY: bool = false;

#[inline(always)]
unsafe fn sync_dma_cache(ptr: *const u8, len: usize) {
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

unsafe fn find_xhci_controller() -> Option<usize> {
    for bus in 0..=255u8 {
        for dev in 0..32u8 {
            for func in 0..8u8 {
                let id = pci_read32(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF || id == 0 {
                    continue;
                }
                let class_rev = pci_read32(bus, dev, func, 0x08);
                let class = (class_rev >> 24) as u8;
                let subclass = (class_rev >> 16) as u8;
                let prog_if = (class_rev >> 8) as u8;

                if class == 0x0C && subclass == 0x03 && prog_if == 0x30 {
                    let cmd = pci_read32(bus, dev, func, 0x04);
                    pci_write32(bus, dev, func, 0x04, (cmd | 0x06) & !(1 << 10));

                    let bar0 = pci_read32(bus, dev, func, 0x10) as u64;
                    let is_64bit = ((bar0 >> 1) & 0x03) == 2;
                    let bar1 = if is_64bit {
                        pci_read32(bus, dev, func, 0x14) as u64
                    } else {
                        0
                    };
                    let mmio = ((bar1 << 32) | (bar0 & !0xF)) as usize;
                    if mmio != 0 {
                        return Some(mmio);
                    }
                }
            }
        }
    }
    None
}

unsafe fn take_bios_ownership(base: usize) {
    let hcc1 = read_volatile((base + 0x10) as *const u32);
    let xecp = ((hcc1 >> 16) & 0xFFFF) as usize * 4;
    if xecp == 0 {
        return;
    }

    let mut cap_addr = base + xecp;
    for _ in 0..32 {
        let cap_hdr = read_volatile(cap_addr as *const u32);
        let cap_id = cap_hdr & 0xFF;
        let next_off = ((cap_hdr >> 8) & 0xFF) as usize * 4;

        if cap_id == 1 {
            write_volatile(cap_addr as *mut u32, cap_hdr | (1 << 24));
            for _ in 0..2_000_000 {
                let v = read_volatile(cap_addr as *const u32);
                if (v & (1 << 16)) == 0 {
                    break;
                }
                asm!("pause");
            }
            let leg_ctl = (cap_addr + 4) as *mut u32;
            let ctl_val = read_volatile(leg_ctl);
            write_volatile(leg_ctl, ctl_val & 0x000E_0000);
            break;
        }

        if next_off == 0 {
            break;
        }
        cap_addr += next_off;
    }
}

unsafe fn wait_event(expected_type: u32) -> Option<(u8, u32)> {
    let evt_ring = EVENT_RING.0.as_ptr() as *const u32;
    for _ in 0..2_000_000 {
        let etrb = evt_ring.add(EVT_DEQ_IDX * 4);
        sync_dma_cache(etrb as *const u8, 16);
        let status = read_volatile(etrb.add(3));
        if (status & 1) == EVT_CYCLE {
            let trb_type = (status >> 10) & 0x3F;
            let comp_code = (read_volatile(etrb.add(2)) >> 24) & 0xFF;
            let slot_id = ((status >> 24) & 0xFF) as u8;

            EVT_DEQ_IDX += 1;
            if EVT_DEQ_IDX >= 256 {
                EVT_DEQ_IDX = 0;
                EVT_CYCLE ^= 1;
            }
            let ir0 = XHCI_RTS_OFF + 0x20;
            let next_erdp = (EVENT_RING.0.as_ptr() as u64) + ((EVT_DEQ_IDX * 16) as u64);
            write_volatile((ir0 + 0x18) as *mut u64, next_erdp | (1 << 3));

            if trb_type == expected_type {
                return Some((slot_id, comp_code));
            }
        }
        asm!("pause");
    }
    None
}

unsafe fn send_command(trb0: u32, trb1: u32, trb2: u32, trb3: u32) -> Option<(u8, u32)> {
    let ring = CMD_RING.0.as_mut_ptr() as *mut u32;
    let trb = ring.add(CMD_ENQ_IDX * 4);

    write_volatile(trb.add(0), trb0);
    write_volatile(trb.add(1), trb1);
    write_volatile(trb.add(2), trb2);
    write_volatile(trb.add(3), (trb3 & !1) | CMD_CYCLE);
    sync_dma_cache(trb as *const u8, 16);

    CMD_ENQ_IDX += 1;
    if CMD_ENQ_IDX >= 250 {
        let link = ring.add(CMD_ENQ_IDX * 4);
        let base = CMD_RING.0.as_ptr() as u64;
        write_volatile(link.add(0), base as u32);
        write_volatile(link.add(1), (base >> 32) as u32);
        write_volatile(link.add(2), 0);
        write_volatile(link.add(3), (6 << 10) | (1 << 1) | CMD_CYCLE);
        sync_dma_cache(link as *const u8, 16);
        CMD_ENQ_IDX = 0;
        CMD_CYCLE ^= 1;
    }

    write_volatile(XHCI_DB_OFF as *mut u32, 0);
    wait_event(33)
}

unsafe fn push_ep0_trb(trb0: u32, trb1: u32, trb2: u32, trb3: u32) {
    let ring = EP0_RING.0.as_mut_ptr() as *mut u32;
    let trb = ring.add(EP0_ENQ_IDX * 4);
    write_volatile(trb.add(0), trb0);
    write_volatile(trb.add(1), trb1);
    write_volatile(trb.add(2), trb2);
    write_volatile(trb.add(3), (trb3 & !1) | EP0_CYCLE);
    sync_dma_cache(trb as *const u8, 16);

    EP0_ENQ_IDX += 1;
    if EP0_ENQ_IDX >= 250 {
        let link = ring.add(EP0_ENQ_IDX * 4);
        let base = EP0_RING.0.as_ptr() as u64;
        write_volatile(link.add(0), base as u32);
        write_volatile(link.add(1), (base >> 32) as u32);
        write_volatile(link.add(2), 0);
        write_volatile(link.add(3), (6 << 10) | (1 << 1) | EP0_CYCLE);
        sync_dma_cache(link as *const u8, 16);
        EP0_ENQ_IDX = 0;
        EP0_CYCLE ^= 1;
    }
}

unsafe fn send_control_no_data(slot: u8, bm_request: u8, b_request: u8, w_value: u16, w_index: u16) -> bool {
    let trb0 = (bm_request as u32) | ((b_request as u32) << 8) | ((w_value as u32) << 16);
    let trb1 = w_index as u32;
    push_ep0_trb(trb0, trb1, 8, (2 << 10) | (1 << 6));
    push_ep0_trb(0, 0, 0, (4 << 10) | (1 << 16) | (1 << 5));

    let db = (XHCI_DB_OFF + (slot as usize) * 4) as *mut u32;
    write_volatile(db, 1);

    if let Some((_, comp)) = wait_event(32) {
        return comp == 1 || comp == 13;
    }
    false
}

unsafe fn send_control_in(slot: u8, bm_request: u8, b_request: u8, w_value: u16, w_index: u16, len: u16) -> bool {
    let buf_addr = DESC_BUF.0.as_mut_ptr() as u64;
    for i in 0..256 {
        write_volatile(DESC_BUF.0.as_mut_ptr().add(i), 0);
    }
    sync_dma_cache(DESC_BUF.0.as_ptr(), 256);

    let trb0 = (bm_request as u32) | ((b_request as u32) << 8) | ((w_value as u32) << 16);
    let trb1 = (w_index as u32) | ((len as u32) << 16);
    push_ep0_trb(trb0, trb1, 8, (2 << 10) | (1 << 6) | (3 << 16));
    push_ep0_trb(buf_addr as u32, (buf_addr >> 32) as u32, len as u32, (3 << 10) | (1 << 16));
    push_ep0_trb(0, 0, 0, (4 << 10) | (1 << 5));

    let db = (XHCI_DB_OFF + (slot as usize) * 4) as *mut u32;
    write_volatile(db, 1);

    if let Some((_, comp)) = wait_event(32) {
        sync_dma_cache(DESC_BUF.0.as_ptr(), 256);
        return comp == 1 || comp == 13;
    }
    false
}

unsafe fn parse_hid_mouse_descriptor(total_len: usize) -> Option<(u8, u8, u32, u32)> {
    let ptr = DESC_BUF.0.as_ptr();
    let len = total_len.min(256);
    let mut off = 0usize;
    let mut in_mouse_iface = false;
    let mut iface_num = 0u8;
    let mut config_val = 1u8;

    while off + 2 <= len {
        let d_len = read_volatile(ptr.add(off)) as usize;
        let d_type = read_volatile(ptr.add(off + 1));
        if d_len < 2 || off + d_len > len {
            break;
        }

        if d_type == 2 && d_len >= 9 {
            config_val = read_volatile(ptr.add(off + 5));
        } else if d_type == 4 && d_len >= 9 {
            let b_iface = read_volatile(ptr.add(off + 2));
            let b_class = read_volatile(ptr.add(off + 5));
            let b_sub = read_volatile(ptr.add(off + 6));
            let b_proto = read_volatile(ptr.add(off + 7));
            if b_class == 0x03 && b_sub == 0x01 && b_proto == 0x02 {
                in_mouse_iface = true;
                iface_num = b_iface;
            } else {
                in_mouse_iface = false;
            }
        } else if d_type == 5 && d_len >= 7 && in_mouse_iface {
            let ep_addr = read_volatile(ptr.add(off + 2));
            let attrs = read_volatile(ptr.add(off + 3));
            let mps_lo = read_volatile(ptr.add(off + 4)) as u32;
            let mps_hi = read_volatile(ptr.add(off + 5)) as u32;
            let mps = mps_lo | (mps_hi << 8);
            if (ep_addr & 0x80) != 0 && (attrs & 0x03) == 3 {
                let ep_num = (ep_addr & 0x0F) as u32;
                let dci = ep_num * 2 + 1;
                return Some((config_val, iface_num, dci, mps.max(4).min(64)));
            }
        }
        off += d_len;
    }
    None
}
unsafe fn queue_hid_interrupt_in() {
    let ring = EP_INT_RING.0.as_mut_ptr() as *mut u32;
    let trb = ring.add(EP_INT_ENQ_IDX * 4);
    let buf_addr = HID_BUF.0.as_mut_ptr() as u64;
    write_volatile(trb.add(0), buf_addr as u32);
    write_volatile(trb.add(1), (buf_addr >> 32) as u32);
    write_volatile(trb.add(2), MOUSE_MPS);
    write_volatile(trb.add(3), (1 << 10) | (1 << 5) | EP_INT_CYCLE);
    sync_dma_cache(trb as *const u8, 16);
    EP_INT_ENQ_IDX += 1;
    if EP_INT_ENQ_IDX >= 250 {
        let link = ring.add(EP_INT_ENQ_IDX * 4);
        let base_addr = EP_INT_RING.0.as_ptr() as u64;
        write_volatile(link.add(0), base_addr as u32);
        write_volatile(link.add(1), (base_addr >> 32) as u32);
        write_volatile(link.add(2), 0);
        write_volatile(link.add(3), (6 << 10) | (1 << 1) | EP_INT_CYCLE);
        sync_dma_cache(link as *const u8, 16);
        EP_INT_ENQ_IDX = 0;
        EP_INT_CYCLE ^= 1;
    }
    if ACTIVE_SLOT > 0 && XHCI_DB_OFF != 0 {
        let db = (XHCI_DB_OFF + (ACTIVE_SLOT as usize) * 4) as *mut u32;
        write_volatile(db, MOUSE_DCI);
    }
}
pub unsafe fn init_usb_mouse() -> bool {
    let Some(base) = find_xhci_controller() else {
        return false;
    };
    XHCI_MMIO = base;
    take_bios_ownership(base);
    let cap_len = read_volatile(base as *const u8) as usize;
    let hcs1 = read_volatile((base + 0x04) as *const u32);
    let hcc1 = read_volatile((base + 0x10) as *const u32);
    CTX_SIZE_64 = (hcc1 & (1 << 2)) != 0;
    let op_base = base + cap_len;
    XHCI_DB_OFF = base + (read_volatile((base + 0x14) as *const u32) as usize);
    XHCI_RTS_OFF = base + (read_volatile((base + 0x18) as *const u32) as usize);
    let usbcmd = (op_base + 0x00) as *mut u32;
    let usbsts = (op_base + 0x04) as *const u32;
    write_volatile(usbcmd, read_volatile(usbcmd) & !1);
    for _ in 0..500_000 {
        if (read_volatile(usbsts) & 1) != 0 {
            break;
        }
    }
    write_volatile(usbcmd, 1 << 1);
    for _ in 0..1_000_000 {
        if (read_volatile(usbcmd) & (1 << 1)) == 0 && (read_volatile(usbsts) & (1 << 11)) == 0 {
            break;
        }
    }
    write_volatile((op_base + 0x38) as *mut u32, 32);
    write_volatile((op_base + 0x30) as *mut u64, DCBAA.0.as_ptr() as u64);
    write_volatile((op_base + 0x18) as *mut u64, (CMD_RING.0.as_ptr() as u64) | 1);
    let ir0 = XHCI_RTS_OFF + 0x20;
    let erst_ptr = ERST.0.as_mut_ptr() as *mut u64;
    write_volatile(erst_ptr, EVENT_RING.0.as_ptr() as u64);
    write_volatile(erst_ptr.add(1), 256);
    sync_dma_cache(ERST.0.as_ptr(), 64);
    write_volatile((ir0 + 0x08) as *mut u32, 1);
    write_volatile((ir0 + 0x18) as *mut u64, EVENT_RING.0.as_ptr() as u64);
    write_volatile((ir0 + 0x10) as *mut u64, ERST.0.as_ptr() as u64);
    write_volatile(usbcmd, 1);
    for _ in 0..500_000 {
        if (read_volatile(usbsts) & 1) == 0 {
            break;
        }
    }
    let max_ports = ((hcs1 >> 24) & 0xFF) as usize;
    let ctx_stride = if CTX_SIZE_64 { 16 } else { 8 };
    for port in 0..max_ports {
        let portsc = (op_base + 0x400 + port * 0x10) as *mut u32;
        let val = read_volatile(portsc);
        if (val & 1) == 0 {
            continue;
        }
        if (val & (1 << 1)) == 0 {
            write_volatile(portsc, (val & 0x0E00_C3E0) | (1 << 4));
            for _ in 0..500_000 {
                if (read_volatile(portsc) & (1 << 1)) != 0 {
                    break;
                }
            }
        }
        let st = read_volatile(portsc);
        if (st & (1 << 1)) == 0 {
            continue;
        }
        let port_num = (port + 1) as u32;
        let port_speed = (st >> 10) & 0x0F;
        let Some((slot_id, comp)) = send_command(0, 0, 0, 9 << 10) else {
            continue;
        };
        if comp != 1 || slot_id == 0 {
            continue;
        }
        let dcbaa_entries = DCBAA.0.as_mut_ptr() as *mut u64;
        write_volatile(dcbaa_entries.add(slot_id as usize), OUTPUT_CTX.0.as_ptr() as u64);
        sync_dma_cache(dcbaa_entries as *const u8, 256);
        EP0_ENQ_IDX = 0;
        EP0_CYCLE = 1;
        for i in 0..4096 {
            write_volatile(EP0_RING.0.as_mut_ptr().add(i), 0);
        }
        sync_dma_cache(EP0_RING.0.as_ptr(), 4096);
        let in_ctx = INPUT_CTX.0.as_mut_ptr() as *mut u32;
        for i in 0..512 {
            write_volatile(in_ctx.add(i), 0);
        }
        write_volatile(in_ctx.add(1), 0x03);
        let slot_ctx = in_ctx.add(ctx_stride);
        write_volatile(slot_ctx.add(0), (port_speed << 20) | (1 << 27));
        write_volatile(slot_ctx.add(1), port_num << 16);
        let mps0 = if port_speed == 4 { 512 } else if port_speed == 3 { 64 } else { 8 };
        let ep0_ctx = in_ctx.add(ctx_stride * 2);
        let ep0_ptr = (EP0_RING.0.as_ptr() as u64) | 1;
        write_volatile(ep0_ctx.add(1), (3 << 1) | (4 << 3) | (mps0 << 16));
        write_volatile(ep0_ctx.add(2), ep0_ptr as u32);
        write_volatile(ep0_ctx.add(3), (ep0_ptr >> 32) as u32);
        write_volatile(ep0_ctx.add(4), 8);
        sync_dma_cache(INPUT_CTX.0.as_ptr(), 2048);
        let in_addr = INPUT_CTX.0.as_ptr() as u64;
        let Some((_, addr_comp)) = send_command(
            in_addr as u32,
            (in_addr >> 32) as u32,
                                                0,
                                                (11 << 10) | ((slot_id as u32) << 24),
        ) else {
            let _ = send_command(0, 0, 0, (10 << 10) | ((slot_id as u32) << 24));
            continue;
        };
        if addr_comp != 1 {
            let _ = send_command(0, 0, 0, (10 << 10) | ((slot_id as u32) << 24));
            continue;
        }
        if send_control_in(slot_id, 0x80, 0x06, 0x0200, 0, 9) {
            let d_ptr = DESC_BUF.0.as_ptr();
            let total_len = (read_volatile(d_ptr.add(2)) as u16) | ((read_volatile(d_ptr.add(3)) as u16) << 8);
            if send_control_in(slot_id, 0x80, 0x06, 0x0200, 0, total_len.min(256)) {
                if let Some((cfg_val, iface_num, dci, ep_mps)) = parse_hid_mouse_descriptor(total_len as usize) {
                    ACTIVE_SLOT = slot_id;
                    MOUSE_DCI = dci;
                    MOUSE_MPS = ep_mps;
                    for i in 0..512 {
                        write_volatile(in_ctx.add(i), 0);
                    }
                    write_volatile(in_ctx.add(1), 1 | (1 << dci));
                    write_volatile(slot_ctx.add(0), (port_speed << 20) | (dci << 27));
                    write_volatile(slot_ctx.add(1), port_num << 16);
                    let ep_ctx = in_ctx.add(ctx_stride * (dci as usize + 1));
                    let ep_ptr = (EP_INT_RING.0.as_ptr() as u64) | 1;
                    write_volatile(ep_ctx.add(0), 4 << 16);
                    write_volatile(ep_ctx.add(1), (3 << 1) | (7 << 3) | (ep_mps << 16));
                    write_volatile(ep_ctx.add(2), ep_ptr as u32);
                    write_volatile(ep_ctx.add(3), (ep_ptr >> 32) as u32);
                    write_volatile(ep_ctx.add(4), ep_mps);
                    sync_dma_cache(INPUT_CTX.0.as_ptr(), 2048);
                    if let Some((_, cfg_comp)) = send_command(
                        in_addr as u32,
                        (in_addr >> 32) as u32,
                                                              0,
                                                              (12 << 10) | ((slot_id as u32) << 24),
                    ) {
                        if cfg_comp == 1 {
                            let _ = send_control_no_data(slot_id, 0x00, 0x09, cfg_val as u16, 0);
                            let _ = send_control_no_data(slot_id, 0x21, 0x0B, 0, iface_num as u16);
                            USB_MOUSE_READY = true;
                            queue_hid_interrupt_in();
                            return true;
                        }
                    }
                }
            }
        }
        let _ = send_command(0, 0, 0, (10 << 10) | ((slot_id as u32) << 24));
    }
    false
}
pub unsafe fn poll_usb_mouse() -> Option<UsbMouseEvent> {
    if !USB_MOUSE_READY || XHCI_RTS_OFF == 0 {
        return None;
    }
    let evt_ring = EVENT_RING.0.as_ptr() as *const u32;
    let trb = evt_ring.add(EVT_DEQ_IDX * 4);
    sync_dma_cache(trb as *const u8, 16);
    let status = read_volatile(trb.add(3));
    if (status & 1) != EVT_CYCLE {
        return None;
    }
    let trb_type = (status >> 10) & 0x3F;
    let mut result: Option<UsbMouseEvent> = None;
    if trb_type == 32 {
        let buf_ptr = HID_BUF.0.as_ptr();
        sync_dma_cache(buf_ptr, 64);
        let buttons = read_volatile(buf_ptr);
        let dx = read_volatile(buf_ptr.add(1)) as i8;
        let dy = read_volatile(buf_ptr.add(2)) as i8;
        result = Some(UsbMouseEvent { dx, dy, buttons });
        queue_hid_interrupt_in();
    }
    EVT_DEQ_IDX += 1;
    if EVT_DEQ_IDX >= 256 {
        EVT_DEQ_IDX = 0;
        EVT_CYCLE ^= 1;
    }
    let ir0 = XHCI_RTS_OFF + 0x20;
    let next_trb_addr = (EVENT_RING.0.as_ptr() as u64) + ((EVT_DEQ_IDX * 16) as u64);
    write_volatile((ir0 + 0x18) as *mut u64, next_trb_addr | (1 << 3));
    result
}
