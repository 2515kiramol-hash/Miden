#![allow(dead_code)]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::write_volatile;

static mut SAVED_FB: *mut u32 = core::ptr::null_mut();
static mut SAVED_W: isize = 1024;
static mut SAVED_H: isize = 768;

pub unsafe fn register_rdm_fb(fb: *mut u32, w: isize, h: isize) {
    SAVED_FB = fb;
    SAVED_W = w;
    SAVED_H = h;
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    val
}

unsafe fn fill_rect(fb: *mut u32, stride: isize, max_h: isize, x: isize, y: isize, w: isize, h: isize, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let mut dy = 0;
    while dy < h {
        let py = y + dy;
        if py >= 0 && py < max_h {
            let row = fb.offset(py * stride);
            let mut dx = 0;
            while dx < w {
                let px = x + dx;
                if px >= 0 && px < stride {
                    write_volatile(row.offset(px), color);
                }
                dx += 1;
            }
        }
        dy += 1;
    }
}

pub unsafe fn show_rdm(exit_code: &str) -> ! {
    // Выключаем звук PC Speaker
    let spk = inb(0x61) & 0xFC;
    outb(0x61, spk);

    let fb = SAVED_FB;
    let w = SAVED_W;
    let h = SAVED_H;

    if fb.is_null() || w <= 0 || h <= 0 {
        loop {
            outb(0x64, 0xFE);
            asm!("hlt");
        }
    }

    let bg_red = 0x007F1D1D;
    let dark_red = 0x00450A0A;
    let border_red = 0x00EF4444;

    fill_rect(fb, w, h, 0, 0, w, h, bg_red);
    fill_rect(fb, w, h, 0, 0, w, 8, border_red);
    fill_rect(fb, w, h, 0, h - 8, w, 8, border_red);

    let left = 96isize;
    let top = 120isize;

    crate::kernels::kernel_64::draw_txt(fb, left, top, "Miden has crashet!", 0x00FFFFFF);
    crate::kernels::kernel_64::draw_txt(fb, left + 1, top, "Miden has crashet!", 0x00FFFFFF);
    crate::kernels::kernel_64::draw_txt(fb, left, top + 1, "Miden has crashet!", 0x00FCA5A5);

    crate::kernels::kernel_64::draw_txt(
        fb,
        left,
        top + 42,
        "A system failure has occurred, and we need to restart the system.",
        0x00FEE2E2,
    );
    crate::kernels::kernel_64::draw_txt(
        fb,
        left,
        top + 62,
        "You can see the error code below.",
        0x00FEE2E2,
    );

    fill_rect(fb, w, h, left, top + 96, 560, 34, dark_red);
    fill_rect(fb, w, h, left, top + 96, 4, 34, border_red);
    crate::kernels::kernel_64::draw_txt(fb, left + 14, top + 107, "Exit code:", 0x00FCA5A5);
    crate::kernels::kernel_64::draw_txt(fb, left + 104, top + 107, exit_code, 0x00FACC15);

    let rdm_y = top + 240;
    crate::kernels::kernel_64::draw_txt(fb, left, rdm_y, "---------------------------", 0x00FCA5A5);
    crate::kernels::kernel_64::draw_txt(fb, left, rdm_y + 20, "RDM(RED DEATH MIDEN)", 0x00FFFFFF);
    crate::kernels::kernel_64::draw_txt(fb, left + 1, rdm_y + 20, "RDM(RED DEATH MIDEN)", 0x00FFFFFF);
    crate::kernels::kernel_64::draw_txt(fb, left, rdm_y + 40, "---------------------------", 0x00FCA5A5);

    let mut pct: u32 = 0;
    while pct <= 100 {
        let prog_y = top + 160;
        fill_rect(fb, w, h, left, prog_y, 420, 24, bg_red);

        let mut line = *b"Rebooting.... 000%";
        let p_ptr = line.as_mut_ptr();
        if pct >= 100 {
            write_volatile(p_ptr.add(14), b'1');
            write_volatile(p_ptr.add(15), b'0');
            write_volatile(p_ptr.add(16), b'0');
            write_volatile(p_ptr.add(17), b'%');
        } else if pct >= 10 {
            write_volatile(p_ptr.add(14), b'0' + ((pct / 10) as u8));
            write_volatile(p_ptr.add(15), b'0' + ((pct % 10) as u8));
            write_volatile(p_ptr.add(16), b'%');
            write_volatile(p_ptr.add(17), b' ');
        } else {
            write_volatile(p_ptr.add(14), b'0' + (pct as u8));
            write_volatile(p_ptr.add(15), b'%');
            write_volatile(p_ptr.add(16), b' ');
            write_volatile(p_ptr.add(17), b' ');
        }

        if let Ok(s) = core::str::from_utf8(&line) {
            crate::kernels::kernel_64::draw_txt(fb, left, prog_y + 4, s, 0x00FFFFFF);
        }

        let bar_w = 340isize;
        let fill_w = (bar_w * (pct as isize)) / 100;
        fill_rect(fb, w, h, left, prog_y + 28, bar_w, 10, dark_red);
        fill_rect(fb, w, h, left, prog_y + 28, fill_w, 10, 0x00FFFFFF);

        let mut delay = 0u32;
        while delay < 1_800_000 {
            if (delay & 0x3FFF) == 0 && (inb(0x64) & 1) != 0 {
                let st = inb(0x64);
                let sc = inb(0x60);
                if (st & 0x20) == 0 && sc == 0x1C {
                    pct = 100;
                    break;
                }
            }
            asm!("pause");
            delay += 1;
        }

        pct += 2;
    }

    loop {
        outb(0x64, 0xFE);
        asm!("hlt");
    }
}

pub unsafe fn handle_rust_panic(_info: &PanicInfo) -> ! {
    show_rdm("0x0000DEAD (RUST_KERNEL_PANIC_RING0)");
}
