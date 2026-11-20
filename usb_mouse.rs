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
static mut EP1_RING: DmaPage = DmaPage([0; 4096]);

#[repr(align(64))]
struct HidBuffer([u8; 64]);
static mut HID_BUF: HidBuffer = HidBuffer([0; 64]);

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

static mut EP1_ENQ_IDX: usize = 0;
static mut EP1_CYCLE: u32 = 1;

static mut ACTIVE_SLOT: u8 = 0;
static mut USB_MOUSE_READY: bool = false;

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
                    pci_write32(bus, dev, func, 0x04, cmd | 0x06);

                    let bar0 = pci_read32(bus, dev, func, 0x10) as usize;
                    let bar1 = pci_read32(bus, dev, func, 0x14) as usize;
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

unsafe fn send_command(trb0: u32, trb1: u32, trb2: u32, trb3: u32) -> Option<(u8, u32)> {
    let ring = CMD_RING.0.as_mut_ptr() as *mut u32;
    let trb = ring.add(CMD_ENQ_IDX * 4);

    write_volatile(trb.add(0), trb0);
    write_volatile(trb.add(1), trb1);
    write_volatile(trb.add(2), trb2);
    write_volatile(trb.add(3), (trb3 & !1) | CMD_CYCLE);

    CMD_ENQ_IDX += 1;
    if CMD_ENQ_IDX >= 250 {
        let link = ring.add(CMD_ENQ_IDX * 4);
        let base = CMD_RING.0.as_ptr() as u64;
        write_volatile(link.add(0), base as u32);
        write_volatile(link.add(1), (base >> 32) as u32);
        write_volatile(link.add(2), 0);
        write_volatile(link.add(3), (6 << 10) | (1 << 1) | CMD_CYCLE);
        CMD_ENQ_IDX = 0;
        CMD_CYCLE ^= 1;
    }

    write_volatile(XHCI_DB_OFF as *mut u32, 0);

    let evt_ring = EVENT_RING.0.as_ptr() as *const u32;
    for _ in 0..1_000_000 {
        let etrb = evt_ring.add(EVT_DEQ_IDX * 4);
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

            if trb_type == 33 {
                return Some((slot_id, comp_code));
            }
        }
        asm!("pause");
    }
    None
}

unsafe fn send_control_no_data(slot: u8, bm_request: u8, b_request: u8, w_value: u16, w_index: u16) -> bool {
    let ring = EP0_RING.0.as_mut_ptr() as *mut u32;

    let setup = ring.add(EP0_ENQ_IDX * 4);
    let trb0 = (bm_request as u32) | ((b_request as u32) << 8) | ((w_value as u32) << 16);
    let trb1 = w_index as u32;
    write_volatile(setup.add(0), trb0);
    write_volatile(setup.add(1), trb1);
    write_volatile(setup.add(2), 8);
    write_volatile(setup.add(3), (2 << 10) | (1 << 6) | EP0_CYCLE);
    EP0_ENQ_IDX += 1;

    let status = ring.add(EP0_ENQ_IDX * 4);
    write_volatile(status.add(0), 0);
    write_volatile(status.add(1), 0);
    write_volatile(status.add(2), 0);
    write_volatile(status.add(3), (4 << 10) | (1 << 16) | (1 << 5) | EP0_CYCLE);
    EP0_ENQ_IDX += 1;

    let db = (XHCI_DB_OFF + (slot as usize) * 4) as *mut u32;
    write_volatile(db, 1);

    let evt_ring = EVENT_RING.0.as_ptr() as *const u32;
    for _ in 0..1_000_000 {
        let etrb = evt_ring.add(EVT_DEQ_IDX * 4);
        let st = read_volatile(etrb.add(3));
        if (st & 1) == EVT_CYCLE {
            let trb_type = (st >> 10) & 0x3F;
            let comp_code = (read_volatile(etrb.add(2)) >> 24) & 0xFF;

            EVT_DEQ_IDX += 1;
            if EVT_DEQ_IDX >= 256 {
                EVT_DEQ_IDX = 0;
                EVT_CYCLE ^= 1;
            }
            let ir0 = XHCI_RTS_OFF + 0x20;
            let next_erdp = (EVENT_RING.0.as_ptr() as u64) + ((EVT_DEQ_IDX * 16) as u64);
            write_volatile((ir0 + 0x18) as *mut u64, next_erdp | (1 << 3));

            if trb_type == 32 {
                return comp_code == 1 || comp_code == 13;
            }
        }
        asm!("pause");
    }
    false
}

unsafe fn queue_hid_interrupt_in() {
    let ring = EP1_RING.0.as_mut_ptr() as *mut u32;
    let trb = ring.add(EP1_ENQ_IDX * 4);
    let buf_addr = HID_BUF.0.as_mut_ptr() as u64;

    write_volatile(trb.add(0), buf_addr as u32);
    write_volatile(trb.add(1), (buf_addr >> 32) as u32);
    write_volatile(trb.add(2), 4);
    write_volatile(trb.add(3), (1 << 10) | (1 << 5) | EP1_CYCLE);

    EP1_ENQ_IDX += 1;
    if EP1_ENQ_IDX >= 250 {
        let link = ring.add(EP1_ENQ_IDX * 4);
        let base_addr = EP1_RING.0.as_ptr() as u64;
        write_volatile(link.add(0), base_addr as u32);
        write_volatile(link.add(1), (base_addr >> 32) as u32);
        write_volatile(link.add(2), 0);
        write_volatile(link.add(3), (6 << 10) | (1 << 1) | EP1_CYCLE);
        EP1_ENQ_IDX = 0;
        EP1_CYCLE ^= 1;
    }

    if ACTIVE_SLOT > 0 && XHCI_DB_OFF != 0 {
        let db = (XHCI_DB_OFF + (ACTIVE_SLOT as usize) * 4) as *mut u32;
        write_volatile(db, 3);
    }
}

pub unsafe fn init_usb_mouse() -> bool {
    let Some(base) = find_xhci_controller() else {
        return false;
    };
    XHCI_MMIO = base;

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
    for _ in 0..100_000 {
        if (read_volatile(usbsts) & 1) != 0 {
            break;
        }
    }

    write_volatile(usbcmd, 1 << 1);
    for _ in 0..200_000 {
        if (read_volatile(usbcmd) & (1 << 1)) == 0 && (read_volatile(usbsts) & (1 << 11)) == 0 {
            break;
        }
    }

    write_volatile((op_base + 0x38) as *mut u32, 16);
    write_volatile((op_base + 0x30) as *mut u64, DCBAA.0.as_ptr() as u64);
    write_volatile((op_base + 0x18) as *mut u64, (CMD_RING.0.as_ptr() as u64) | 1);

    let ir0 = XHCI_RTS_OFF + 0x20;
    let erst_ptr = ERST.0.as_mut_ptr() as *mut u64;
    write_volatile(erst_ptr, EVENT_RING.0.as_ptr() as u64);
    write_volatile(erst_ptr.add(1), 256);

    write_volatile((ir0 + 0x08) as *mut u32, 1);
    write_volatile((ir0 + 0x18) as *mut u64, EVENT_RING.0.as_ptr() as u64);
    write_volatile((ir0 + 0x10) as *mut u64, ERST.0.as_ptr() as u64);

    write_volatile(usbcmd, 1);
    for _ in 0..100_000 {
        if (read_volatile(usbsts) & 1) == 0 {
            break;
        }
    }

    let max_ports = ((hcs1 >> 24) & 0xFF) as usize;
    let mut found_port = 0u32;
    let mut port_speed = 0u32;

    for port in 0..max_ports {
        let portsc = (op_base + 0x400 + port * 0x10) as *mut u32;
        let val = read_volatile(portsc);
        if (val & 1) != 0 {
            write_volatile(portsc, (val & 0x0E00_C3E0) | (1 << 4));
            for _ in 0..200_000 {
                let st = read_volatile(portsc);
                if (st & (1 << 1)) != 0 {
                    found_port = (port + 1) as u32;
                    port_speed = (st >> 10) & 0x0F;
                    break;
                }
            }
            if found_port != 0 {
                break;
            }
        }
    }
    if found_port == 0 {
        return false;
    }
    let Some((slot_id, comp)) = send_command(0, 0, 0, 9 << 10) else {
        return false;
    };
    if comp != 1 || slot_id == 0 {
        return false;
    }
    ACTIVE_SLOT = slot_id;
    let dcbaa_entries = DCBAA.0.as_mut_ptr() as *mut u64;
    write_volatile(dcbaa_entries.add(slot_id as usize), OUTPUT_CTX.0.as_ptr() as u64);
    let ctx_stride = if CTX_SIZE_64 { 16 } else { 8 };
    let in_ctx = INPUT_CTX.0.as_mut_ptr() as *mut u32;
    for i in 0..256 {
        write_volatile(in_ctx.add(i), 0);
    }
    write_volatile(in_ctx.add(1), 0x03);
    let slot_ctx = in_ctx.add(ctx_stride);
    write_volatile(slot_ctx.add(0), (port_speed << 20) | (1 << 27));
    write_volatile(slot_ctx.add(1), found_port << 16);
    let mps = if port_speed == 4 { 512 } else if port_speed == 3 { 64 } else { 8 };
    let ep0_ctx = in_ctx.add(ctx_stride * 2);
    let ep0_ptr = (EP0_RING.0.as_ptr() as u64) | 1;
    write_volatile(ep0_ctx.add(1), (3 << 1) | (4 << 3) | (mps << 16));
    write_volatile(ep0_ctx.add(2), ep0_ptr as u32);
    write_volatile(ep0_ctx.add(3), (ep0_ptr >> 32) as u32);
    write_volatile(ep0_ctx.add(4), 8);
    let in_addr = INPUT_CTX.0.as_ptr() as u64;
    let Some((_, addr_comp)) = send_command(
        in_addr as u32,
        (in_addr >> 32) as u32,
                                            0,
                                            (11 << 10) | ((slot_id as u32) << 24),
    ) else {
        return false;
    };
    if addr_comp != 1 {
        return false;
    }
    for i in 0..256 {
        write_volatile(in_ctx.add(i), 0);
    }
    write_volatile(in_ctx.add(1), 0x09);
    write_volatile(slot_ctx.add(0), (port_speed << 20) | (3 << 27));
    write_volatile(slot_ctx.add(1), found_port << 16);
    let ep1_ctx = in_ctx.add(ctx_stride * 4);
    let ep1_ptr = (EP1_RING.0.as_ptr() as u64) | 1;
    write_volatile(ep1_ctx.add(0), 4 << 16);
    write_volatile(ep1_ctx.add(1), (3 << 1) | (7 << 3) | (8 << 16));
    write_volatile(ep1_ctx.add(2), ep1_ptr as u32);
    write_volatile(ep1_ctx.add(3), (ep1_ptr >> 32) as u32);
    write_volatile(ep1_ctx.add(4), 8);
    let Some((_, cfg_comp)) = send_command(
        in_addr as u32,
        (in_addr >> 32) as u32,
                                           0,
                                           (12 << 10) | ((slot_id as u32) << 24),
    ) else {
        return false;
    };
    if cfg_comp != 1 {
        return false;
    }
    let _ = send_control_no_data(slot_id, 0x00, 0x09, 1, 0);
    let _ = send_control_no_data(slot_id, 0x21, 0x0B, 0, 0);
    USB_MOUSE_READY = true;
    queue_hid_interrupt_in();
    true
}
pub unsafe fn poll_usb_mouse() -> Option {
    if !USB_MOUSE_READY || XHCI_RTS_OFF == 0 {
        return None;
    }
    let evt_ring = EVENT_RING.0.as_ptr() as *const u32;
    let trb = evt_ring.add(EVT_DEQ_IDX * 4);
    let status = read_volatile(trb.add(3));
    if (status & 1) != EVT_CYCLE {
        return None;
    }
    let trb_type = (status >> 10) & 0x3F;
    let mut result = None;
    if trb_type == 32 {
        let buttons = read_volatile(&HID_BUF.0[0]);
        let dx = read_volatile(&HID_BUF.0[1]) as i8;
        let dy = read_volatile(&HID_BUF.0[2]) as i8;
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
