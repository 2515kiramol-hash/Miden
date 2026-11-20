#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

static mut SAVED_FB: *mut u32 = core::ptr::null_mut();
static mut SAVED_W: isize = 1024;
static mut SAVED_H: isize = 768;

static mut SLEEP_SNAPSHOT: [u32; 1920 * 1080] = [0; 1920 * 1080];
static mut POWER_WORK_BUF: [u32; 1920 * 1080] = [0; 1920 * 1080];

pub unsafe fn register_power_fb(fb: *mut u32, w: isize, h: isize) {
    SAVED_FB = fb;
    SAVED_W = w;
    SAVED_H = h;
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
}

#[inline(always)]
unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    val
}

#[inline(always)]
fn scale_pixel(col: u32, factor: u32) -> u32 {
    let r = (((col >> 16) & 0xFF) * factor) >> 8;
    let g = (((col >> 8) & 0xFF) * factor) >> 8;
    let b = ((col & 0xFF) * factor) >> 8;
    (r << 16) | (g << 8) | b
}

unsafe fn flush_buf_to_vram(src: *const u32, dst: *mut u32, total_px: usize) {
    let qwords = total_px / 2;
    let s64 = src as *const u64;
    let d64 = dst as *mut u64;
    let mut i = 0usize;
    while i < qwords {
        write_volatile(d64.add(i), read_volatile(s64.add(i)));
        i += 1;
    }
    if (total_px & 1) != 0 {
        write_volatile(dst.add(total_px - 1), read_volatile(src.add(total_px - 1)));
    }
}

unsafe fn copy_vram_to_buf(vram: *const u32, dst: *mut u32, total_px: usize) {
    let mut i = 0usize;
    while i < total_px {
        write_volatile(dst.add(i), read_volatile(vram.add(i)));
        i += 1;
    }
}

/// Плавное затухание экрана и завершение работы системы
pub unsafe fn smooth_shutdown() -> ! {
    crate::sound::IS_PLAYING = false;
    crate::sound::stop_sound();

    let fb = SAVED_FB;
    let w = SAVED_W;
    let h = SAVED_H;

    if !fb.is_null() && w > 0 && h > 0 {
        let total_px = ((w * h) as usize).min(1920 * 1080);
        let snap_ptr = SLEEP_SNAPSHOT.as_mut_ptr();
        let work_ptr = POWER_WORK_BUF.as_mut_ptr();

        copy_vram_to_buf(fb, snap_ptr, total_px);

        let mut brightness: i32 = 256;
        while brightness >= 0 {
            let f = brightness as u32;
            let mut i = 0usize;
            while i < total_px {
                let px = read_volatile(snap_ptr.add(i));
                write_volatile(work_ptr.add(i), scale_pixel(px, f));
                i += 1;
            }

            if brightness > 20 {
                let text_col = scale_pixel(0x00F8FAFC, f.max(90));
                let sub_col = scale_pixel(0x0038BDF8, f.max(90));
                crate::kernels::kernel_64::draw_txt(
                    work_ptr,
                    w / 2 - 116,
                    h / 2 - 12,
                    "Завершение работы midenOS...",
                    text_col,
                );
                crate::kernels::kernel_64::draw_txt(
                    work_ptr,
                    w / 2 - 136,
                    h / 2 + 12,
                    "Остановка служб и парковка дисков",
                    sub_col,
                );
            }

            flush_buf_to_vram(work_ptr, fb, total_px);

            let mut d = 0u32;
            while d < 450_000 {
                asm!("pause");
                d += 1;
            }
            brightness -= 4;
        }

        let mut k = 0usize;
        while k < total_px {
            write_volatile(work_ptr.add(k), 0x00000000);
            k += 1;
        }
        flush_buf_to_vram(work_ptr, fb, total_px);
    }

    let mut wait = 0u32;
    while wait < 12_000_000 {
        asm!("pause");
        wait += 1;
    }

    outw(0x604, 0x2000);
    outw(0xB004, 0x2000);
    outw(0x4004, 0x3400);

    if !fb.is_null() && w > 0 && h > 0 {
        crate::kernels::kernel_64::draw_txt(
            fb,
            w / 2 - 196,
            h / 2,
            "Теперь питание компьютера можно безопасно отключить.",
            0x00FACC15,
        );
    }

    loop {
        asm!("cli; hlt");
    }
}

/// Режим сна с плавным затемнением и пробуждением по мыши или клавиатуре
pub unsafe fn enter_sleep_mode() {
    crate::sound::IS_PLAYING = false;
    crate::sound::stop_sound();

    let fb = SAVED_FB;
    let w = SAVED_W;
    let h = SAVED_H;
    if fb.is_null() || w <= 0 || h <= 0 {
        return;
    }

    let total_px = ((w * h) as usize).min(1920 * 1080);
    let snap_ptr = SLEEP_SNAPSHOT.as_mut_ptr();
    let work_ptr = POWER_WORK_BUF.as_mut_ptr();

    copy_vram_to_buf(fb, snap_ptr, total_px);

    let mut b: i32 = 256;
    while b >= 0 {
        let f = b as u32;
        let mut i = 0usize;
        while i < total_px {
            write_volatile(work_ptr.add(i), scale_pixel(read_volatile(snap_ptr.add(i)), f));
            i += 1;
        }
        if b > 30 {
            crate::kernels::kernel_64::draw_txt(
                work_ptr,
                w / 2 - 92,
                h / 2 - 8,
                "Переход в режим сна...",
                scale_pixel(0x0038BDF8, f),
            );
        }
        flush_buf_to_vram(work_ptr, fb, total_px);
        let mut d = 0u32;
        while d < 250_000 {
            asm!("pause");
            d += 1;
        }
        b -= 8;
    }

    while (inb(0x64) & 1) != 0 {
        let _ = inb(0x60);
    }

    let mut sleep_ticks: u32 = 0;
    loop {
        if (inb(0x64) & 1) != 0 {
            let _ = inb(0x60);
            break;
        }

        if sleep_ticks % 20 == 0 {
            let mut k = 0usize;
            while k < total_px {
                write_volatile(work_ptr.add(k), 0x00000000);
                k += 1;
            }
            if sleep_ticks < 240 {
                let pulse = ((sleep_ticks / 20) % 2) == 0;
                let col = if pulse { 0x00334155 } else { 0x001E293B };
                crate::kernels::kernel_64::draw_txt(
                    work_ptr,
                    w / 2 - 148,
                    h / 2,
                    "zZz  Режим сна (двиньте мышью для выхода)",
                                                    col,
                );
            }
            flush_buf_to_vram(work_ptr, fb, total_px);
        }

        let mut d = 0u32;
        while d < 300_000 {
            asm!("pause");
            d += 1;
        }
        sleep_ticks = sleep_ticks.saturating_add(1);
    }

    let mut wake: i32 = 0;
    while wake <= 256 {
        let f = wake.min(256) as u32;
        let mut i = 0usize;
        while i < total_px {
            write_volatile(work_ptr.add(i), scale_pixel(read_volatile(snap_ptr.add(i)), f));
            i += 1;
        }
        flush_buf_to_vram(work_ptr, fb, total_px);
        let mut d = 0u32;
        while d < 200_000 {
            asm!("pause");
            d += 1;
        }
        wake += 16;
    }
}
