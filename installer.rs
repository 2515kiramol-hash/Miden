// kernel/src/installer.rs — Windows-style 64-bit Setup Wizard с wallpaper.bmp, мышью и AHCI/IDE (HP 250 G1)
use core::arch::asm;
use core::ptr;
use crate::kernels::kernel_64::{
    draw_txt, init_ps2_mouse, init_wallpaper_embedded, parse_multiboot2,
    try_draw_wallpaper_bmp, FB_PITCH, REAL_FB, SCREEN_H, SCREEN_W,
};

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack, preserves_flags));
    val
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    let val: u16;
    asm!("in ax, dx", out("ax") val, in("dx") port, options(nomem, nostack, preserves_flags));
    val
}

#[inline(always)]
unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    asm!("in eax, dx", out("eax") val, in("dx") port, options(nomem, nostack, preserves_flags));
    val
}

#[inline(always)]
unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn mmio_read32(addr: usize) -> u32 {
    let p = addr as *const u32;
    ptr::read_volatile(p)
}

#[inline(always)]
unsafe fn mmio_write32(addr: usize, val: u32) {
    let p = addr as *mut u32;
    ptr::write_volatile(p, val);
}

unsafe fn pci_read32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
    | ((bus as u32) << 16)
    | ((dev as u32) << 11)
    | ((func as u32) << 8)
    | ((offset as u32) & 0xFC);
    outl(0xCF8, addr);
    inl(0xCFC)
}

unsafe fn pci_write32(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    let addr = 0x8000_0000u32
    | ((bus as u32) << 16)
    | ((dev as u32) << 11)
    | ((func as u32) << 8)
    | ((offset as u32) & 0xFC);
    outl(0xCF8, addr);
    outl(0xCFC, val);
}

#[derive(Copy, Clone, PartialEq)]
enum DiskKind {
    None,
    Ahci,
    Ide,
}

#[derive(Copy, Clone)]
struct DiskDevice {
    kind: DiskKind,
    ahci_abar: usize,
    ahci_port: u8,
    ide_base: u16,
    ide_slave: bool,
    sectors: u64,
    model: [u8; 40],
}

impl DiskDevice {
    const fn empty() -> Self {
        Self {
            kind: DiskKind::None,
            ahci_abar: 0,
            ahci_port: 0,
            ide_base: 0,
            ide_slave: false,
            sectors: 0,
            model: [b' '; 40],
        }
    }
}

#[repr(C, align(1024))]
struct AhciCmdList {
    headers: [AhciCmdHeader; 32],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct AhciCmdHeader {
    flags: u16,
    prdtl: u16,
    prdbc: u32,
    ctba: u32,
    ctbau: u32,
    rsv: [u32; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct AhciPrdtEntry {
    dba: u32,
    dbau: u32,
    rsv: u32,
    dbc: u32,
}

#[repr(C, align(128))]
struct AhciCmdTable {
    cfis: [u8; 64],
    acmd: [u8; 16],
    rsv: [u8; 48],
    prdt: [AhciPrdtEntry; 1],
}

#[repr(C, align(256))]
struct AhciFis {
    data: [u8; 256],
}

#[repr(align(16))]
struct DmaSectorBuf {
    data: [u8; 512],
}

static mut AHCI_CL: AhciCmdList = AhciCmdList {
    headers: [AhciCmdHeader { flags: 0, prdtl: 0, prdbc: 0, ctba: 0, ctbau: 0, rsv: [0; 4] }; 32],
};
static mut AHCI_CT: AhciCmdTable = AhciCmdTable {
    cfis: [0; 64],
    acmd: [0; 16],
    rsv: [0; 48],
    prdt: [AhciPrdtEntry { dba: 0, dbau: 0, rsv: 0, dbc: 0 }],
};
static mut AHCI_FIS: AhciFis = AhciFis { data: [0; 256] };
static mut DMA_BUF: DmaSectorBuf = DmaSectorBuf { data: [0; 512] };
static mut DISKS: [DiskDevice; 8] = [DiskDevice::empty(); 8];
static mut DISK_COUNT: usize = 0;

unsafe fn parse_identify_model(words: &[u16; 256], out_model: &mut [u8; 40]) {
    for i in 0..20 {
        let w = *words.get_unchecked(27 + i);
        let c1 = (w >> 8) as u8;
        let c2 = (w & 0xFF) as u8;
        *out_model.get_unchecked_mut(i * 2) = if c1 >= 32 && c1 <= 126 { c1 } else { b' ' };
        *out_model.get_unchecked_mut(i * 2 + 1) = if c2 >= 32 && c2 <= 126 { c2 } else { b' ' };
    }
}

unsafe fn ahci_port_rw(abar: usize, port: u8, fis_cmd: u8, lba: u64, write: bool, buf: *mut u8) -> bool {
    let pbase = abar + 0x100 + (port as usize) * 0x80;

    let mut cmd = mmio_read32(pbase + 0x18);
    cmd &= !(1 << 0);
    cmd &= !(1 << 4);
    mmio_write32(pbase + 0x18, cmd);
    for _ in 0..200_000 {
        let c = mmio_read32(pbase + 0x18);
        if (c & (1 << 15)) == 0 && (c & (1 << 14)) == 0 { break; }
    }

    if write {
        ptr::copy_nonoverlapping(buf, (*ptr::addr_of_mut!(DMA_BUF)).data.as_mut_ptr(), 512);
    }

    let cl_addr = ptr::addr_of_mut!(AHCI_CL) as u64;
    let fis_addr = ptr::addr_of_mut!(AHCI_FIS) as u64;
    let ct_addr = ptr::addr_of_mut!(AHCI_CT) as u64;
    let dma_addr = (*ptr::addr_of_mut!(DMA_BUF)).data.as_ptr() as u64;

    mmio_write32(pbase + 0x00, cl_addr as u32);
    mmio_write32(pbase + 0x04, (cl_addr >> 32) as u32);
    mmio_write32(pbase + 0x08, fis_addr as u32);
    mmio_write32(pbase + 0x0C, (fis_addr >> 32) as u32);

    let hdr = (*ptr::addr_of_mut!(AHCI_CL)).headers.get_unchecked_mut(0);
    hdr.flags = 5 | if write { 1 << 6 } else { 0 };
    hdr.prdtl = 1;
    hdr.prdbc = 0;
    hdr.ctba = ct_addr as u32;
    hdr.ctbau = (ct_addr >> 32) as u32;

    let ct = &mut *ptr::addr_of_mut!(AHCI_CT);
    for b in ct.cfis.iter_mut() { *b = 0; }
    *ct.cfis.get_unchecked_mut(0) = 0x27;
    *ct.cfis.get_unchecked_mut(1) = 1 << 7;
    *ct.cfis.get_unchecked_mut(2) = fis_cmd;
    *ct.cfis.get_unchecked_mut(4) = (lba & 0xFF) as u8;
    *ct.cfis.get_unchecked_mut(5) = ((lba >> 8) & 0xFF) as u8;
    *ct.cfis.get_unchecked_mut(6) = ((lba >> 16) & 0xFF) as u8;
    *ct.cfis.get_unchecked_mut(7) = 1 << 6;
    *ct.cfis.get_unchecked_mut(8) = ((lba >> 24) & 0xFF) as u8;
    *ct.cfis.get_unchecked_mut(9) = ((lba >> 32) & 0xFF) as u8;
    *ct.cfis.get_unchecked_mut(10) = ((lba >> 40) & 0xFF) as u8;
    *ct.cfis.get_unchecked_mut(12) = 1;

    let prdt0 = ct.prdt.get_unchecked_mut(0);
    prdt0.dba = dma_addr as u32;
    prdt0.dbau = (dma_addr >> 32) as u32;
    prdt0.rsv = 0;
    prdt0.dbc = 511;

    mmio_write32(pbase + 0x30, 0xFFFF_FFFF);
    mmio_write32(pbase + 0x10, 0xFFFF_FFFF);

    cmd = mmio_read32(pbase + 0x18);
    cmd |= 1 << 4;
    mmio_write32(pbase + 0x18, cmd);
    for _ in 0..5000 { asm!("pause"); }

    cmd |= 1 << 0;
    mmio_write32(pbase + 0x18, cmd);

    for _ in 0..500_000 {
        let tfd = mmio_read32(pbase + 0x20);
        if (tfd & 0x88) == 0 { break; }
    }

    mmio_write32(pbase + 0x38, 1);

    let mut ok = false;
    for _ in 0..2_500_000 {
        let ci = mmio_read32(pbase + 0x38);
        let is = mmio_read32(pbase + 0x10);
        if (is & (1 << 30)) != 0 { return false; }
        if (ci & 1) == 0 {
            ok = true;
            break;
        }
    }

    if ok && !write {
        ptr::copy_nonoverlapping((*ptr::addr_of!(DMA_BUF)).data.as_ptr(), buf, 512);
    }
    ok
}

unsafe fn ide_wait_ready(base: u16) -> bool {
    for _ in 0..200_000 {
        let st = inb(base + 7);
        if (st & 0x80) == 0 { return true; }
    }
    false
}

unsafe fn ide_wait_drq(base: u16) -> bool {
    for _ in 0..200_000 {
        let st = inb(base + 7);
        if (st & 0x01) != 0 { return false; }
        if (st & 0x80) == 0 && (st & 0x08) != 0 { return true; }
    }
    false
}

unsafe fn ide_identify(base: u16, slave: bool) -> Option<DiskDevice> {
    outb(base + 6, if slave { 0xB0 } else { 0xA0 });
    for _ in 0..1000 { inb(base + 7); }
    outb(base + 2, 0);
    outb(base + 3, 0);
    outb(base + 4, 0);
    outb(base + 5, 0);
    outb(base + 7, 0xEC);
    let st = inb(base + 7);
    if st == 0 || st == 0xFF { return None; }
    if !ide_wait_drq(base) { return None; }

    let mut words = [0u16; 256];
    for i in 0..256 { *words.get_unchecked_mut(i) = inw(base); }

    let mut dev = DiskDevice::empty();
    dev.kind = DiskKind::Ide;
    dev.ide_base = base;
    dev.ide_slave = slave;
    parse_identify_model(&words, &mut dev.model);

    let lba48 = (*words.get_unchecked(100) as u64)
    | ((*words.get_unchecked(101) as u64) << 16)
    | ((*words.get_unchecked(102) as u64) << 32)
    | ((*words.get_unchecked(103) as u64) << 48);
    let lba28 = (*words.get_unchecked(60) as u64) | ((*words.get_unchecked(61) as u64) << 16);
    dev.sectors = if lba48 > 0 { lba48 } else { lba28 };
    if dev.sectors == 0 { return None; }
    Some(dev)
}

unsafe fn ide_rw_sector(base: u16, slave: bool, lba: u32, write: bool, buf: *mut u8) -> bool {
    if !ide_wait_ready(base) { return false; }
    let drive_head = if slave { 0xF0 } else { 0xE0 } | (((lba >> 24) & 0x0F) as u8);
    outb(base + 6, drive_head);
    outb(base + 2, 1);
    outb(base + 3, (lba & 0xFF) as u8);
    outb(base + 4, ((lba >> 8) & 0xFF) as u8);
    outb(base + 5, ((lba >> 16) & 0xFF) as u8);
    outb(base + 7, if write { 0x30 } else { 0x20 });

    if !ide_wait_drq(base) { return false; }
    let ptr16 = buf as *mut u16;
    if write {
        for i in 0..256 { outw(base, *ptr16.add(i)); }
        outb(base + 7, 0xE7);
        let _ = ide_wait_ready(base);
    } else {
        for i in 0..256 { *ptr16.add(i) = inw(base); }
    }
    true
}

unsafe fn disk_write_sector(dev: &DiskDevice, lba: u64, buf: *const u8) -> bool {
    match dev.kind {
        DiskKind::Ahci => ahci_port_rw(dev.ahci_abar, dev.ahci_port, 0x35, lba, true, buf as *mut u8),
        DiskKind::Ide => ide_rw_sector(dev.ide_base, dev.ide_slave, lba as u32, true, buf as *mut u8),
        DiskKind::None => false,
    }
}

unsafe fn scan_all_disks() {
    DISK_COUNT = 0;
    for bus in 0u8..=255 {
        for dev in 0u8..32 {
            let id0 = pci_read32(bus, dev, 0, 0x00);
            if (id0 & 0xFFFF) == 0xFFFF { continue; }
            let hdr_type = ((pci_read32(bus, dev, 0, 0x0C) >> 16) & 0xFF) as u8;
            let max_func = if (hdr_type & 0x80) != 0 { 8u8 } else { 1u8 };
            for func in 0u8..max_func {
                let id = pci_read32(bus, dev, func, 0x00);
                if (id & 0xFFFF) == 0xFFFF { continue; }
                let class_reg = pci_read32(bus, dev, func, 0x08);
                let base_class = ((class_reg >> 24) & 0xFF) as u8;
                let sub_class = ((class_reg >> 16) & 0xFF) as u8;
                if base_class == 0x01 && (sub_class == 0x06 || sub_class == 0x04) {
                    let cmd = pci_read32(bus, dev, func, 0x04);
                    pci_write32(bus, dev, func, 0x04, cmd | 0x06);
                    let abar = (pci_read32(bus, dev, func, 0x24) & 0xFFFF_F000) as usize;
                    if abar != 0 {
                        let ghc = mmio_read32(abar + 0x04);
                        mmio_write32(abar + 0x04, ghc | (1 << 31));
                        let pi = mmio_read32(abar + 0x0C);
                        for port in 0u8..32 {
                            if (pi & (1 << port)) != 0 {
                                let pbase = abar + 0x100 + (port as usize) * 0x80;
                                let ssts = mmio_read32(pbase + 0x28);
                                let sig = mmio_read32(pbase + 0x24);
                                let det = ssts & 0x0F;
                                if det == 3 && sig != 0xEB14_0101 {
                                    let mut raw = [0u8; 512];
                                    if ahci_port_rw(abar, port, 0xEC, 0, false, raw.as_mut_ptr()) {
                                        let mut words = [0u16; 256];
                                        ptr::copy_nonoverlapping(raw.as_ptr(), words.as_mut_ptr() as *mut u8, 512);
                                        let mut d = DiskDevice::empty();
                                        d.kind = DiskKind::Ahci;
                                        d.ahci_abar = abar;
                                        d.ahci_port = port;
                                        parse_identify_model(&words, &mut d.model);
                                        let lba48 = (*words.get_unchecked(100) as u64)
                                        | ((*words.get_unchecked(101) as u64) << 16)
                                        | ((*words.get_unchecked(102) as u64) << 32)
                                        | ((*words.get_unchecked(103) as u64) << 48);
                                        let lba28 = (*words.get_unchecked(60) as u64) | ((*words.get_unchecked(61) as u64) << 16);
                                        d.sectors = if lba48 > 0 { lba48 } else { lba28 };
                                        if d.sectors > 0 && DISK_COUNT < 8 {
                                            *DISKS.get_unchecked_mut(DISK_COUNT) = d;
                                            DISK_COUNT += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if bus == 255 { break; }
    }
    let channels: [(u16, bool); 4] = [
        (0x1F0, false),
        (0x1F0, true),
        (0x170, false),
        (0x170, true),
    ];
    for &(base, slave) in channels.iter() {
        if DISK_COUNT >= 8 { break; }
        if let Some(d) = ide_identify(base, slave) {
            *DISKS.get_unchecked_mut(DISK_COUNT) = d;
            DISK_COUNT += 1;
        }
    }
}
unsafe fn fill_rect(fb: *mut u32, x: isize, y: isize, w: isize, h: isize, color: u32) {
    for dy in 0..h {
        let py = y + dy;
        if py < 0 || py >= SCREEN_H { continue; }
        let row = fb.offset(py * SCREEN_W);
        for dx in 0..w {
            let px = x + dx;
            if px >= 0 && px < SCREEN_W {
                *row.offset(px) = color;
            }
        }
    }
}
unsafe fn draw_border(fb: *mut u32, x: isize, y: isize, w: isize, h: isize, color: u32) {
    fill_rect(fb, x, y, w, 1, color);
    fill_rect(fb, x, y + h - 1, w, 1, color);
    fill_rect(fb, x, y, 1, h, color);
    fill_rect(fb, x + w - 1, y, 1, h, color);
}
unsafe fn draw_button(fb: *mut u32, x: isize, y: isize, w: isize, h: isize, label: &str, primary: bool, hovered: bool) {
    let bg = if primary {
        if hovered { 0x002B88D8 } else { 0x000078D4 }
    } else {
        if hovered { 0x00383E4A } else { 0x002A2F3A }
    };
    let border = if primary { 0x0060CDFF } else { 0x00525B6B };
    fill_rect(fb, x, y, w, h, bg);
    draw_border(fb, x, y, w, h, border);
    let tx = x + (w - (label.len() as isize) * 8) / 2;
    let ty = y + (h - 12) / 2;
    draw_txt(fb, tx.max(x + 6), ty, label, 0x00FFFFFF);
}
unsafe fn draw_cursor(fb: *mut u32, mx: isize, my: isize) {
    let shape: [&[u8]; 14] = [
        b"X.............",
        b"XX............",
        b"X.X...........",
        b"X..X..........",
        b"X...X.........",
        b"X....X........",
        b"X.....X.......",
        b"X......X......",
        b"X.......X.....",
        b"X.....XXXX....",
        b"X..X..X.......",
        b"X.X.X..X......",
        b"XX..X..X......",
        b"X....XX.......",
    ];
    for (dy, row) in shape.iter().enumerate() {
        let py = my + dy as isize;
        if py < 0 || py >= SCREEN_H { continue; }
        for (dx, &ch) in row.iter().enumerate() {
            let px = mx + dx as isize;
            if px < 0 || px >= SCREEN_W { continue; }
            if ch == b'X' {
                *fb.offset(py * SCREEN_W + px) = 0x00FFFFFF;
            } else if ch == b'.' && dx > 0 && dy > 0 && row.get(dx - 1) == Some(&b'X') {
                *fb.offset(py * SCREEN_W + px) = 0x00000000;
            }
        }
    }
}
unsafe fn flush_screen(back: *const u32) {
    let pitch_u32 = if FB_PITCH > 0 { (FB_PITCH / 4) as usize } else { SCREEN_W as usize };
    for y in 0..(SCREEN_H as usize) {
        let dst = REAL_FB.add(y * pitch_u32);
        let src = back.add(y * (SCREEN_W as usize));
        ptr::copy_nonoverlapping(src, dst, SCREEN_W as usize);
    }
}
fn in_rect(mx: isize, my: isize, x: isize, y: isize, w: isize, h: isize) -> bool {
    mx >= x && mx < x + w && my >= y && my < y + h
}
fn scancode_to_ascii(sc: u8, shift: bool) -> char {
    let normal: &[u8] = &[
        0, 27, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', 8, 9,
        b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', 10, 0,
        b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', 39, b'`', 0, 92,
        b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', 0, 42, 0, b' ',
    ];
    let shifted: &[u8] = &[
        0, 27, b'!', b'@', b'#', b'$', b'%', b'^', b'&', 42, b'(', b')', b'_', b'+', 8, 9,
        b'Q', b'W', b'E', b'R', b'T', b'Y', b'U', b'I', b'O', b'P', b'{', b'}', 10, 0,
        b'A', b'S', b'D', b'F', b'G', b'H', b'J', b'K', b'L', b':', 34, b'~', 0, b'|',
        b'Z', b'X', b'C', b'V', b'B', b'N', b'M', b'<', b'>', b'?', 0, 42, 0, b' ',
    ];
    let idx = sc as usize;
    if idx < normal.len() {
        let b = if shift { shifted[idx] } else { normal[idx] };
        if b >= 32 && b <= 126 { return b as char; }
    }
    '\0'
}
unsafe fn format_u64(mut val: u64, buf: &mut [u8; 24]) -> &str {
    if val == 0 {
        *buf.get_unchecked_mut(0) = b'0';
        return core::str::from_utf8(&buf[..1]).unwrap_or("0");
    }
    let mut tmp = [0u8; 24];
    let mut i = 0;
    while val > 0 {
        *tmp.get_unchecked_mut(i) = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    for j in 0..i {
        *buf.get_unchecked_mut(j) = *tmp.get_unchecked(i - 1 - j);
    }
    core::str::from_utf8(&buf[..i]).unwrap_or("0")
}
unsafe fn run_windows_install_process(
    fb: *mut u32,
    bg_cache: *const u32,
    disk: &DiskDevice,
    user: &[u8],
    pass: &[u8],
    pc_name: &[u8],
) -> bool {
    let draw_install_ui = |stage: usize, pct: isize, status_txt: &str| {
        ptr::copy_nonoverlapping(bg_cache, fb, (SCREEN_W * SCREEN_H) as usize);
        let wx = 172;
        let wy = 114;
        let ww = 680;
        let wh = 540;
        fill_rect(fb, wx, wy, ww, wh, 0x001B1F27);
        draw_border(fb, wx, wy, ww, wh, 0x000078D4);
        fill_rect(fb, wx, wy, ww, 42, 0x000078D4);
        draw_txt(fb, wx + 18, wy + 15, "Установка midenOS - Копирование и настройка системы", 0x00FFFFFF);
        draw_txt(fb, wx + 36, wy + 70, "Установка midenOS на компьютер", 0x0060CDFF);
        draw_txt(fb, wx + 36, wy + 96, "Не выключайте компьютер. Это займет несколько секунд.", 0x00A0AAB8);
        let steps = [
            "Подготовка целевого накопителя и разметка MBR",
            "Копирование системных файлов ядра midenOS (64-bit)",
            "Установка компонентов и графической оболочки",
            "Создание учетной записи пользователя и параметров ПК",
            "Завершение установки и очистка буферов",
        ];
        for (i, label) in steps.iter().enumerate() {
            let sy = wy + 150 + (i as isize) * 38;
            if i < stage {
                draw_txt(fb, wx + 44, sy, "[OK]", 0x003FB950);
                draw_txt(fb, wx + 95, sy, label, 0x00E6EDF3);
            } else if i == stage {
                draw_txt(fb, wx + 44, sy, "[>>]", 0x0060CDFF);
                draw_txt(fb, wx + 95, sy, label, 0x00FFFFFF);
            } else {
                draw_txt(fb, wx + 44, sy, "[  ]", 0x006E7681);
                draw_txt(fb, wx + 95, sy, label, 0x006E7681);
            }
        }
        fill_rect(fb, wx + 36, wy + 380, ww - 72, 26, 0x00262C36);
        draw_border(fb, wx + 36, wy + 380, ww - 72, 26, 0x003D4452);
        let fill_w = ((ww - 76) * pct) / 100;
        if fill_w > 0 {
            fill_rect(fb, wx + 38, wy + 382, fill_w, 22, 0x000078D4);
        }
        draw_txt(fb, wx + 36, wy + 418, status_txt, 0x00C9D1D9);
        flush_screen(fb);
    };
    draw_install_ui(0, 8, "Установка загрузочного MBR, Stage-2 и splash.bmp...");
    if !crate::mbr::install_mbr_and_splash(disk.sectors, |lba, ptr| {
        disk_write_sector(disk, lba, ptr)
    }) {
        return false;
    }
    let total_sectors: u64 = 10800;
    let kernel_mem = 0x0010_0000 as *const u8;
    for s in 0..total_sectors {
        let src_ptr = kernel_mem.add((s as usize) * 512);
        if !disk_write_sector(disk, 2048 + s, src_ptr) {
            return false;
        }
        if s % 128 == 0 {
            let pct = 15 + ((s * 65) / total_sectors) as isize;
            let st = if pct < 50 { 1 } else { 2 };
            draw_install_ui(st, pct, "Копирование системных файлов на диск...");
        }
    }
    draw_install_ui(3, 88, "Сохранение имени пользователя, пароля и имени ПК...");
    let mut user_sec = [0u8; 512];
    user_sec[0..8].copy_from_slice(b"MIDENUSR");
    *user_sec.get_unchecked_mut(8) = user.len() as u8;
    for i in 0..user.len().min(32) { *user_sec.get_unchecked_mut(16 + i) = *user.get_unchecked(i); }
    *user_sec.get_unchecked_mut(9) = pass.len() as u8;
    for i in 0..pass.len().min(32) { *user_sec.get_unchecked_mut(64 + i) = *pass.get_unchecked(i); }
    *user_sec.get_unchecked_mut(10) = pc_name.len() as u8;
    for i in 0..pc_name.len().min(32) { *user_sec.get_unchecked_mut(128 + i) = *pc_name.get_unchecked(i); }
    if !disk_write_sector(disk, 2040, user_sec.as_ptr()) {
        return false;
    }
    draw_install_ui(4, 100, "Установка успешно завершена!");
    for _ in 0..15_000_000 { asm!("pause"); }
    true
}
pub unsafe fn run_installer(mb_info: u32) -> ! {
    if mb_info != 0 {
        parse_multiboot2(mb_info as usize);
    }
    static mut INST_FB: [u32; (SCREEN_W * SCREEN_H) as usize] = [0; (SCREEN_W * SCREEN_H) as usize];
    static mut BG_CACHE: [u32; (SCREEN_W * SCREEN_H) as usize] = [0; (SCREEN_W * SCREEN_H) as usize];
    let fb = (*ptr::addr_of_mut!(INST_FB)).as_mut_ptr();
    let bg_cache = (*ptr::addr_of_mut!(BG_CACHE)).as_mut_ptr();
    init_wallpaper_embedded();
    if !try_draw_wallpaper_bmp(bg_cache) {
        for y in 0..SCREEN_H {
            let c = 0x000A192F + (((y * 28 / SCREEN_H) as u32) << 8) + ((y * 45 / SCREEN_H) as u32);
            fill_rect(bg_cache, 0, y, SCREEN_W, 1, c);
        }
    }
    init_ps2_mouse();
    ptr::copy_nonoverlapping(bg_cache, fb, (SCREEN_W * SCREEN_H) as usize);
    fill_rect(fb, 262, 310, 500, 120, 0x001B1F27);
    draw_border(fb, 262, 310, 500, 120, 0x000078D4);
    fill_rect(fb, 262, 310, 500, 36, 0x000078D4);
    draw_txt(fb, 280, 322, "Программа установки midenOS", 0x00FFFFFF);
    draw_txt(fb, 300, 372, "Поиск накопителей (SATA AHCI / IDE)...", 0x00E6EDF3);
    flush_screen(fb);
    scan_all_disks();
    let mut step: u8 = 0;
    let mut selected_disk: usize = 0;
    let mut active_field: u8 = 0;
    let mut username = [0u8; 24];
    let mut user_len: usize = 0;
    let mut password = [0u8; 24];
    let mut pass_len: usize = 0;
    let mut pass2 = [0u8; 24];
    let mut pass2_len: usize = 0;
    let mut hostname = [0u8; 24];
    hostname[0..8].copy_from_slice(b"MIDEN-PC");
    let mut host_len: usize = 8;
    let mut err_msg: &str = "";
    let mut install_ok = false;
    let mut shift = false;
    let mut mx: isize = SCREEN_W / 2;
    let mut my: isize = SCREEN_H / 2;
    let mut mouse_left = false;
    let mut mouse_pkt = [0u8; 3];
    let mut mouse_idx: usize = 0;
    let wx: isize = 172;
    let wy: isize = 114;
    let ww: isize = 680;
    let wh: isize = 540;
    loop {
        ptr::copy_nonoverlapping(bg_cache, fb, (SCREEN_W * SCREEN_H) as usize);
        fill_rect(fb, wx, wy, ww, wh, 0x001B1F27);
        draw_border(fb, wx, wy, ww, wh, 0x000078D4);
        fill_rect(fb, wx, wy, ww, 42, 0x000078D4);
        draw_txt(fb, wx + 18, wy + 15, "Установка midenOS (64-bit Setup)", 0x00FFFFFF);
        let close_hov = in_rect(mx, my, wx + ww - 42, wy + 6, 34, 30);
        fill_rect(fb, wx + ww - 42, wy + 6, 34, 30, if close_hov { 0x00E81123 } else { 0x00005A9E });
        draw_txt(fb, wx + ww - 29, wy + 15, "X", 0x00FFFFFF);
        let step_names = ["1. Начало", "2. Выбор диска", "3. Учетная запись", "4. Установка"];
        for (i, sname) in step_names.iter().enumerate() {
            let sx = wx + 24 + (i as isize) * 160;
            let active = (step as usize) == i || (step == 4 && i == 3);
            fill_rect(fb, sx, wy + 54, 150, 26, if active { 0x000078D4 } else { 0x00252B36 });
            draw_txt(fb, sx + 10, wy + 61, sname, if active { 0x00FFFFFF } else { 0x008B949E });
        }
        if step == 0 {
            draw_txt(fb, wx + 44, wy + 115, "Добро пожаловать в программу установки midenOS", 0x0060CDFF);
            draw_txt(fb, wx + 44, wy + 150, "Этот мастер установит 64-битную операционную систему midenOS", 0x00E6EDF3);
            draw_txt(fb, wx + 44, wy + 172, "на ваш жесткий диск или SSD-накопитель.", 0x00E6EDF3);
            fill_rect(fb, wx + 44, wy + 210, ww - 88, 125, 0x00222732);
            draw_border(fb, wx + 44, wy + 210, ww - 88, 125, 0x00363E4D);
            draw_txt(fb, wx + 64, wy + 230, "Язык интерфейса:        Русский (Россия)", 0x00C9D1D9);
            draw_txt(fb, wx + 64, wy + 260, "Раскладка клавиатуры:   США / Русская", 0x00C9D1D9);
            draw_txt(fb, wx + 64, wy + 290, "Архитектура ядра:       x86_64 Long Mode (UEFI / Legacy)", 0x007EE787);
            let btn_x = wx + (ww - 240) / 2;
            let btn_y = wy + 385;
            let hov = in_rect(mx, my, btn_x, btn_y, 240, 48);
            draw_button(fb, btn_x, btn_y, 240, 48, "Установить сейчас", true, hov);
            draw_txt(fb, wx + 195, wy + 455, "Нажмите кнопку мышью или клавишу ENTER", 0x008B949E);
        } else if step == 1 {
            draw_txt(fb, wx + 36, wy + 102, "Куда вы хотите установить midenOS?", 0x0060CDFF);
            fill_rect(fb, wx + 36, wy + 130, ww - 72, 28, 0x00282F3C);
            draw_txt(fb, wx + 48, wy + 138, "Интерфейс", 0x008B949E);
            draw_txt(fb, wx + 165, wy + 138, "Модель накопителя", 0x008B949E);
            draw_txt(fb, wx + 495, wy + 138, "Объем", 0x008B949E);
            fill_rect(fb, wx + 36, wy + 158, ww - 72, 220, 0x0013171E);
            draw_border(fb, wx + 36, wy + 158, ww - 72, 220, 0x00363E4D);
            if DISK_COUNT == 0 {
                draw_txt(fb, wx + 56, wy + 200, "Диски не найдены! Проверьте режим SATA (AHCI/IDE) в BIOS.", 0x00FF7B72);
            } else {
                for i in 0..DISK_COUNT.min(5) {
                    let ry = wy + 164 + (i as isize) * 40;
                    let sel = i == selected_disk;
                    let hov = in_rect(mx, my, wx + 40, ry, ww - 80, 36);
                    let row_bg = if sel { 0x000078D4 } else if hov { 0x00252C38 } else { 0x001B202A };
                    fill_rect(fb, wx + 40, ry, ww - 80, 36, row_bg);
                    let d = DISKS.get_unchecked(i);
                    let kstr = match d.kind {
                        DiskKind::Ahci => "SATA AHCI",
                        DiskKind::Ide => "IDE / ATA",
                        DiskKind::None => "UNKNOWN",
                    };
                    draw_txt(fb, wx + 48, ry + 12, kstr, 0x00FFFFFF);
                    let mstr = core::str::from_utf8(&d.model).unwrap_or("Generic Disk");
                    draw_txt(fb, wx + 165, ry + 12, mstr, 0x00FFFFFF);
                    let mb = d.sectors / 2048;
                    let mut nbuf = [0u8; 24];
                    let mb_s = format_u64(mb, &mut nbuf);
                    draw_txt(fb, wx + 495, ry + 12, mb_s, 0x007EE787);
                    draw_txt(fb, wx + 565, ry + 12, "МБ", 0x007EE787);
                }
            }
            let ref_hov = in_rect(mx, my, wx + 36, wy + 392, 130, 34);
            draw_button(fb, wx + 36, wy + 392, 130, 34, "Обновить", false, ref_hov);
            draw_txt(fb, wx + 36, wy + 440, "Внимание: выбранный диск будет размечен под midenOS.", 0x00D29922);
            let back_hov = in_rect(mx, my, wx + ww - 270, wy + 480, 110, 38);
            let next_hov = in_rect(mx, my, wx + ww - 146, wy + 480, 110, 38);
            draw_button(fb, wx + ww - 270, wy + 480, 110, 38, "< Назад", false, back_hov);
            draw_button(fb, wx + ww - 146, wy + 480, 110, 38, "Далее >", DISK_COUNT > 0, next_hov);
        } else if step == 2 {
            draw_txt(fb, wx + 36, wy + 102, "Создание учетной записи администратора", 0x0060CDFF);
            draw_txt(fb, wx + 36, wy + 128, "Введите имя пользователя и пароль для входа в midenOS:", 0x00A0AAB8);
            let labels = [
                "Имя пользователя:",
                "Пароль:",
                "Подтвердите пароль:",
                "Имя компьютера:",
            ];
            for i in 0..4 {
                let fy = wy + 165 + (i as isize) * 64;
                let act = active_field == i as u8;
                draw_txt(fb, wx + 36, fy, labels[i], 0x00E6EDF3);
                fill_rect(fb, wx + 36, fy + 20, 420, 32, if act { 0x00262D3A } else { 0x0013171E });
                draw_border(fb, wx + 36, fy + 20, 420, 32, if act { 0x0060CDFF } else { 0x003D4452 });
                let stars = [42u8; 24];
                let (txt_slice, len) = match i {
                    0 => (&username[..user_len], user_len),
                    1 => (&stars[..pass_len], pass_len),
                    2 => (&stars[..pass2_len], pass2_len),
                    _ => (&hostname[..host_len], host_len),
                };
                if let Ok(s) = core::str::from_utf8(txt_slice) {
                    draw_txt(fb, wx + 46, fy + 30, s, 0x00FFFFFF);
                }
                if act {
                    draw_txt(fb, wx + 46 + (len as isize) * 8, fy + 30, "_", 0x0060CDFF);
                }
            }
            if !err_msg.is_empty() {
                draw_txt(fb, wx + 36, wy + 432, err_msg, 0x00FF7B72);
            }
            let back_hov = in_rect(mx, my, wx + ww - 290, wy + 480, 110, 38);
            let inst_hov = in_rect(mx, my, wx + ww - 166, wy + 480, 130, 38);
            draw_button(fb, wx + ww - 290, wy + 480, 110, 38, "< Назад", false, back_hov);
            draw_button(fb, wx + ww - 166, wy + 480, 130, 38, "Установить", true, inst_hov);
        } else if step == 3 {
            if user_len == 0 {
                username[0..5].copy_from_slice(b"admin");
                user_len = 5;
            }
            if host_len == 0 {
                hostname[0..8].copy_from_slice(b"MIDEN-PC");
                host_len = 8;
            }
            install_ok = run_windows_install_process(
                fb,
                bg_cache,
                DISKS.get_unchecked(selected_disk),
                                                     &username[..user_len],
                                                     &password[..pass_len],
                                                     &hostname[..host_len],
            );
            step = 4;
            continue;
        } else {
            if install_ok {
                fill_rect(fb, wx + 44, wy + 135, ww - 88, 190, 0x00163325);
                draw_border(fb, wx + 44, wy + 135, ww - 88, 190, 0x003FB950);
                draw_txt(fb, wx + 68, wy + 165, "Установка midenOS успешно завершена!", 0x007EE787);
                draw_txt(fb, wx + 68, wy + 200, "Система и учетная запись пользователя записаны на диск.", 0x00FFFFFF);
                draw_txt(fb, wx + 68, wy + 230, "Извлеките установочную флешку перед перезагрузкой.", 0x00C9D1D9);
                let reb_hov = in_rect(mx, my, wx + (ww - 220) / 2, wy + 380, 220, 46);
                draw_button(fb, wx + (ww - 220) / 2, wy + 380, 220, 46, "Перезагрузить сейчас", true, reb_hov);
            } else {
                fill_rect(fb, wx + 44, wy + 135, ww - 88, 190, 0x003B181C);
                draw_border(fb, wx + 44, wy + 135, ww - 88, 190, 0x00F85149);
                draw_txt(fb, wx + 68, wy + 165, "Сбой при записи на выбранный накопитель!", 0x00FF7B72);
                draw_txt(fb, wx + 68, wy + 200, "Проверьте диск или переключите контроллер SATA в BIOS.", 0x00FFFFFF);
                let ret_hov = in_rect(mx, my, wx + (ww - 220) / 2, wy + 380, 220, 46);
                draw_button(fb, wx + (ww - 220) / 2, wy + 380, 220, 46, "Вернуться к выбору", false, ret_hov);
            }
        }
        draw_cursor(fb, mx, my);
        flush_screen(fb);
        loop {
            let st = inb(0x64);
            if (st & 0x01) == 0 {
                asm!("pause");
                continue;
            }
            let data = inb(0x60);
            if (st & 0x20) != 0 {
                if mouse_idx == 0 && (data & 0x08) == 0 {
                    continue;
                }
                *mouse_pkt.get_unchecked_mut(mouse_idx) = data;
                mouse_idx += 1;
                if mouse_idx == 3 {
                    mouse_idx = 0;
                    let flags = *mouse_pkt.get_unchecked(0);
                    let dx = *mouse_pkt.get_unchecked(1) as i8 as isize;
                    let dy = *mouse_pkt.get_unchecked(2) as i8 as isize;
                    mx = (mx + dx).clamp(0, SCREEN_W - 8);
                    my = (my - dy).clamp(0, SCREEN_H - 8);
                    let prev_mouse_left = mouse_left;
                    mouse_left = (flags & 0x01) != 0;
                    if mouse_left && !prev_mouse_left {
                        if in_rect(mx, my, wx + ww - 42, wy + 6, 34, 30) {
                            crate::kernels::kernel_64::run_graphics(0, 0);
                        }
                        if step == 0 {
                            let btn_x = wx + (ww - 240) / 2;
                            let btn_y = wy + 385;
                            if in_rect(mx, my, btn_x, btn_y, 240, 48) {
                                step = 1;
                            }
                        } else if step == 1 {
                            for i in 0..DISK_COUNT.min(5) {
                                let ry = wy + 164 + (i as isize) * 40;
                                if in_rect(mx, my, wx + 40, ry, ww - 80, 36) {
                                    selected_disk = i;
                                }
                            }
                            if in_rect(mx, my, wx + 36, wy + 392, 130, 34) {
                                scan_all_disks();
                                selected_disk = 0;
                            }
                            if in_rect(mx, my, wx + ww - 270, wy + 480, 110, 38) {
                                step = 0;
                            }
                            if DISK_COUNT > 0 && in_rect(mx, my, wx + ww - 146, wy + 480, 110, 38) {
                                err_msg = "";
                                step = 2;
                            }
                        } else if step == 2 {
                            for i in 0..4 {
                                let fy = wy + 165 + (i as isize) * 64;
                                if in_rect(mx, my, wx + 36, fy + 20, 420, 32) {
                                    active_field = i as u8;
                                }
                            }
                            if in_rect(mx, my, wx + ww - 290, wy + 480, 110, 38) {
                                err_msg = "";
                                step = 1;
                            }
                            if in_rect(mx, my, wx + ww - 166, wy + 480, 130, 38) {
                                if pass_len != pass2_len || password[..pass_len] != pass2[..pass2_len] {
                                    err_msg = "Ошибка: введенные пароли не совпадают!";
                                } else {
                                    err_msg = "";
                                    step = 3;
                                }
                            }
                        } else if step == 4 {
                            if in_rect(mx, my, wx + (ww - 220) / 2, wy + 380, 220, 46) {
                                if install_ok {
                                    outb(0x64, 0xFE);
                                } else {
                                    step = 1;
                                }
                            }
                        }
                    }
                    break;
                }
            } else {
                if data == 0x2A || data == 0x36 { shift = true; continue; }
                if data == 0xAA || data == 0xB6 { shift = false; continue; }
                if (data & 0x80) != 0 { continue; }
                if step == 0 && data == 0x1C {
                    step = 1;
                    break;
                } else if step == 1 {
                    match data {
                        0x48 => { if selected_disk > 0 { selected_disk -= 1; } break; }
                        0x50 => { if selected_disk + 1 < DISK_COUNT { selected_disk += 1; } break; }
                        0x01 => { step = 0; break; }
                        0x1C => { if DISK_COUNT > 0 { err_msg = ""; step = 2; } break; }
                        _ => {}
                    }
                } else if step == 2 {
                    match data {
                        0x0F => { active_field = (active_field + 1) % 4; break; }
                        0x01 => { err_msg = ""; step = 1; break; }
                        0x0E => {
                            match active_field {
                                0 if user_len > 0 => { user_len -= 1; *username.get_unchecked_mut(user_len) = 0; }
                                1 if pass_len > 0 => { pass_len -= 1; *password.get_unchecked_mut(pass_len) = 0; }
                                2 if pass2_len > 0 => { pass2_len -= 1; *pass2.get_unchecked_mut(pass2_len) = 0; }
                                3 if host_len > 0 => { host_len -= 1; *hostname.get_unchecked_mut(host_len) = 0; }
                                _ => {}
                            }
                            break;
                        }
                        0x1C => {
                            if pass_len != pass2_len || password[..pass_len] != pass2[..pass2_len] {
                                err_msg = "Ошибка: введенные пароли не совпадают!";
                            } else {
                                err_msg = "";
                                step = 3;
                            }
                            break;
                        }
                        _ => {
                            let ch = scancode_to_ascii(data, shift);
                            if ch != '\0' {
                                match active_field {
                                    0 if user_len < 20 => { *username.get_unchecked_mut(user_len) = ch as u8; user_len += 1; }
                                    1 if pass_len < 20 => { *password.get_unchecked_mut(pass_len) = ch as u8; pass_len += 1; }
                                    2 if pass2_len < 20 => { *pass2.get_unchecked_mut(pass2_len) = ch as u8; pass2_len += 1; }
                                    3 if host_len < 20 => { *hostname.get_unchecked_mut(host_len) = ch as u8; host_len += 1; }
                                    _ => {}
                                }
                                break;
                            }
                        }
                    }
                } else if step == 4 && data == 0x1C {
                    if install_ok {
                        outb(0x64, 0xFE);
                    } else {
                        step = 1;
                    }
                    break;
                }
            }
        }
    }
}
