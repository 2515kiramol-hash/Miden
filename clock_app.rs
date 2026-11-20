#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

pub const CLOCK_W: isize = 360;
pub const CLOCK_H: isize = 270;

pub static mut WIN_CLOCK: bool = false;
pub static mut CLOCK_X: isize = 260;
pub static mut CLOCK_Y: isize = 120;

static mut CLOCK_TAB: usize = 0; // 0 = Мировое время, 1 = Секундомер, 2 = Таймер
static mut SW_RUNNING: bool = false;
static mut SW_FRAMES: u32 = 0;

static mut TMR_RUNNING: bool = false;
static mut TMR_FRAMES: u32 = 60 * 60; // По умолчанию 1 минута (3600 кадров)

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

unsafe fn read_cmos(reg: u8) -> u8 {
    outb(0x70, reg);
    let raw = inb(0x71);
    ((raw >> 4) * 10) + (raw & 0x0F)
}

unsafe fn fill_rect(fb: *mut u32, stride: isize, x: isize, y: isize, w: isize, h: isize, color: u32) {
    if w <= 0 || h <= 0 { return; }
    let mut dy = 0;
    while dy < h {
        let py = y + dy;
        if py >= 0 && py < 1080 {
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

pub unsafe fn tick_clock_app() {
    if SW_RUNNING {
        SW_FRAMES = SW_FRAMES.saturating_add(1);
    }
    if TMR_RUNNING {
        if TMR_FRAMES > 0 {
            TMR_FRAMES -= 1;
            if TMR_FRAMES == 0 {
                TMR_RUNNING = false;
                crate::sound::start_track(1, true); // Звуковой сигнал окончания таймера
            }
        } else {
            TMR_RUNNING = false;
        }
    }
}

unsafe fn format_mm_ss(total_frames: u32, out: &mut [u8; 8]) {
    let total_sec = total_frames / 60;
    let mins = (total_sec / 60) % 100;
    let secs = total_sec % 60;
    let hundredths = ((total_frames % 60) * 100) / 60;
    let p = out.as_mut_ptr();
    write_volatile(p.add(0), b'0' + ((mins / 10) as u8));
    write_volatile(p.add(1), b'0' + ((mins % 10) as u8));
    write_volatile(p.add(2), b':');
    write_volatile(p.add(3), b'0' + ((secs / 10) as u8));
    write_volatile(p.add(4), b'0' + ((secs % 10) as u8));
    write_volatile(p.add(5), b'.');
    write_volatile(p.add(6), b'0' + ((hundredths / 10) as u8));
    write_volatile(p.add(7), b'0' + ((hundredths % 10) as u8));
}

pub unsafe fn draw_clock_window(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_CLOCK { return; }
    let wx = CLOCK_X;
    let wy = CLOCK_Y;
    let ww = CLOCK_W;
    let wh = CLOCK_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Часы midenOS", 0x0038BDF8);

    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    // 3 вкладки
    let tabs = ["Мировое время", "Секундомер", "Таймер"];
    let mut t = 0isize;
    while t < 3 {
        let tx = wx + 12 + t * 114;
        let act = CLOCK_TAB == (t as usize);
        fill_rect(fb, stride, tx, wy + 34, 108, 24, if act { 0x000284C7 } else { 0x001E293B });
        crate::kernels::kernel_64::draw_txt(fb, tx + 8, wy + 41, tabs[t as usize], 0x00F8FAFC);
        t += 1;
    }

    if CLOCK_TAB == 0 {
        let hr = read_cmos(0x04);
        let min = read_cmos(0x02);
        let sec = read_cmos(0x00);

        let zones: [(&str, i16); 4] = [
            ("Лондон (UTC+0)", 0),
            ("Москва (UTC+3)", 3),
            ("Бузулук / Екб (UTC+5)", 5),
            ("Токио (UTC+9)", 9),
        ];
        let mut i = 0isize;
        while i < 4 {
            let ry = wy + 70 + i * 44;
            fill_rect(fb, stride, wx + 14, ry, ww - 28, 36, 0x001E293B);
            let (name, off) = zones[i as usize];
            crate::kernels::kernel_64::draw_txt(fb, wx + 24, ry + 12, name, 0x00CBD5E1);

            let zh = ((hr as i16 + off).rem_euclid(24)) as u8;
            let mut tstr = *b"00:00:00";
            let tp = tstr.as_mut_ptr();
            write_volatile(tp.add(0), b'0' + (zh / 10));
            write_volatile(tp.add(1), b'0' + (zh % 10));
            write_volatile(tp.add(3), b'0' + (min / 10));
            write_volatile(tp.add(4), b'0' + (min % 10));
            write_volatile(tp.add(6), b'0' + (sec / 10));
            write_volatile(tp.add(7), b'0' + (sec % 10));
            if let Ok(s) = core::str::from_utf8(&tstr) {
                crate::kernels::kernel_64::draw_txt(fb, wx + ww - 96, ry + 12, s, 0x0038BDF8);
            }
            i += 1;
        }
    } else if CLOCK_TAB == 1 {
        fill_rect(fb, stride, wx + 20, wy + 76, ww - 40, 74, 0x00020617);
        let mut buf = [0u8; 8];
        format_mm_ss(SW_FRAMES, &mut buf);
        if let Ok(s) = core::str::from_utf8(&buf) {
            crate::kernels::kernel_64::draw_txt(fb, wx + 140, wy + 106, s, 0x004ADE80);
            crate::kernels::kernel_64::draw_txt(fb, wx + 141, wy + 106, s, 0x004ADE80);
        }

        fill_rect(fb, stride, wx + 40, wy + 174, 130, 32, if SW_RUNNING { 0x00D97706 } else { 0x0016A34A });
        crate::kernels::kernel_64::draw_txt(fb, wx + 82, wy + 185, if SW_RUNNING { "Пауза" } else { "Старт" }, 0x00FFFFFF);

        fill_rect(fb, stride, wx + 190, wy + 174, 130, 32, 0x00334155);
        crate::kernels::kernel_64::draw_txt(fb, wx + 232, wy + 185, "Сброс", 0x00F8FAFC);
    } else {
        fill_rect(fb, stride, wx + 20, wy + 72, ww - 40, 68, 0x00020617);
        let mut buf = [0u8; 8];
        format_mm_ss(TMR_FRAMES, &mut buf);
        if let Ok(s) = core::str::from_utf8(&buf) {
            crate::kernels::kernel_64::draw_txt(fb, wx + 140, wy + 98, s, 0x00FACC15);
            crate::kernels::kernel_64::draw_txt(fb, wx + 141, wy + 98, s, 0x00FACC15);
        }

        // Кнопки +1 мин и +10 сек
        fill_rect(fb, stride, wx + 40, wy + 150, 130, 26, 0x001E293B);
        crate::kernels::kernel_64::draw_txt(fb, wx + 76, wy + 158, "+1 мин", 0x0038BDF8);
        fill_rect(fb, stride, wx + 190, wy + 150, 130, 26, 0x001E293B);
        crate::kernels::kernel_64::draw_txt(fb, wx + 224, wy + 158, "+10 сек", 0x0038BDF8);

        fill_rect(fb, stride, wx + 40, wy + 186, 130, 32, if TMR_RUNNING { 0x00D97706 } else { 0x002563EB });
        crate::kernels::kernel_64::draw_txt(fb, wx + 82, wy + 197, if TMR_RUNNING { "Пауза" } else { "Запуск" }, 0x00FFFFFF);

        fill_rect(fb, stride, wx + 190, wy + 186, 130, 32, 0x00334155);
        crate::kernels::kernel_64::draw_txt(fb, wx + 232, wy + 197, "Сброс", 0x00F8FAFC);
    }
}

pub unsafe fn handle_clock_click(
    mx: isize, my: isize,
    drag_win: &mut u8, drag_off_x: &mut isize, drag_off_y: &mut isize, focused_win: &mut u8,
) -> bool {
    if !WIN_CLOCK { return false; }
    let wx = CLOCK_X;
    let wy = CLOCK_Y;
    let ww = CLOCK_W;
    let wh = CLOCK_H;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh { return false; }

    *focused_win = 15;
    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_CLOCK = false;
        return true;
    }
    if my >= wy && my <= wy + 28 {
        *drag_win = 15;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }
    if my >= wy + 34 && my <= wy + 58 {
        let mut t = 0isize;
        while t < 3 {
            let tx = wx + 12 + t * 114;
            if mx >= tx && mx <= tx + 108 {
                CLOCK_TAB = t as usize;
                return true;
            }
            t += 1;
        }
    }

    if CLOCK_TAB == 1 && my >= wy + 174 && my <= wy + 206 {
        if mx >= wx + 40 && mx <= wx + 170 { SW_RUNNING = !SW_RUNNING; return true; }
        if mx >= wx + 190 && mx <= wx + 320 { SW_RUNNING = false; SW_FRAMES = 0; return true; }
    } else if CLOCK_TAB == 2 {
        if my >= wy + 150 && my <= wy + 176 {
            if mx >= wx + 40 && mx <= wx + 170 { TMR_FRAMES += 3600; return true; }
            if mx >= wx + 190 && mx <= wx + 320 { TMR_FRAMES += 600; return true; }
        }
        if my >= wy + 186 && my <= wy + 218 {
            if mx >= wx + 40 && mx <= wx + 170 { TMR_RUNNING = !TMR_RUNNING; return true; }
            if mx >= wx + 190 && mx <= wx + 320 { TMR_RUNNING = false; TMR_FRAMES = 3600; return true; }
        }
    }

    true
}
