#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

pub const SNIP_W: isize = 400;
pub const SNIP_H: isize = 310;

pub static mut WIN_SNIP: bool = false;
pub static mut SNIP_X: isize = 220;
pub static mut SNIP_Y: isize = 110;

// Режим выделения области поверх экрана
pub static mut SNIP_SELECTING: bool = false;
static mut SEL_DRAGGING: bool = false;
static mut SEL_X1: isize = 0;
static mut SEL_Y1: isize = 0;
static mut SEL_X2: isize = 0;
static mut SEL_Y2: isize = 0;

static mut SCREEN_SNAP: [u32; 1920 * 1080] = [0; 1920 * 1080];
static mut CAPTURED_BUF: [u32; 360 * 190] = [0x00020617; 360 * 190];
static mut CAPTURED_W: isize = 0;
static mut CAPTURED_H: isize = 0;
static mut SAVED_NOTICE: u32 = 0;

unsafe fn fill_rect(fb: *mut u32, stride: isize, x: isize, y: isize, w: isize, h: isize, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
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

/// Запускает режим выделения области экрана мышью
pub unsafe fn start_snip_selection(src_fb: *const u32, scr_w: isize, scr_h: isize) {
    if src_fb.is_null() {
        return;
    }
    let total = ((scr_w * scr_h) as usize).min(1920 * 1080);
    let dst = SCREEN_SNAP.as_mut_ptr();
    let mut i = 0usize;
    while i < total {
        write_volatile(dst.add(i), read_volatile(src_fb.add(i)));
        i += 1;
    }
    SNIP_SELECTING = true;
    SEL_DRAGGING = false;
    SEL_X1 = 0;
    SEL_Y1 = 0;
    SEL_X2 = 0;
    SEL_Y2 = 0;
    WIN_SNIP = false;
}

/// Захватывает выбранный прямоугольник в превью окна "Ножницы"
unsafe fn finalize_capture(scr_w: isize, scr_h: isize, x1: isize, y1: isize, x2: isize, y2: isize) {
    let lx = x1.min(x2).clamp(0, scr_w - 1);
    let rx = x1.max(x2).clamp(0, scr_w);
    let ty = y1.min(y2).clamp(0, scr_h - 1);
    let by = y1.max(y2).clamp(0, scr_h);

    let rw = (rx - lx).max(16);
    let rh = (by - ty).max(16);

    // Масштабируем захваченный фрагмент в окно превью 360x180
    let dst_w = 360isize;
    let dst_h = 180isize;
    let src = SCREEN_SNAP.as_ptr();
    let dst = CAPTURED_BUF.as_mut_ptr();

    let mut dy = 0isize;
    while dy < dst_h {
        let sy = ty + (dy * rh) / dst_h;
        let mut dx = 0isize;
        while dx < dst_w {
            let sx = lx + (dx * rw) / dst_w;
            let col = read_volatile(src.offset(sy * scr_w + sx));
            write_volatile(dst.offset(dy * dst_w + dx), col);
            dx += 1;
        }
        dy += 1;
    }

    CAPTURED_W = rw;
    CAPTURED_H = rh;
    SNIP_SELECTING = false;
    SEL_DRAGGING = false;
    WIN_SNIP = true;
    SAVED_NOTICE = 0;
}

/// Обработка мыши в полноэкранном режиме выделения области
pub unsafe fn handle_snip_overlay_mouse(
    mx: isize,
    my: isize,
    left_down: bool,
    scr_w: isize,
    scr_h: isize,
) -> bool {
    if !SNIP_SELECTING {
        return false;
    }
    if left_down {
        if !SEL_DRAGGING {
            SEL_DRAGGING = true;
            SEL_X1 = mx;
            SEL_Y1 = my;
            SEL_X2 = mx;
            SEL_Y2 = my;
        } else {
            SEL_X2 = mx;
            SEL_Y2 = my;
        }
    } else if SEL_DRAGGING {
        finalize_capture(scr_w, scr_h, SEL_X1, SEL_Y1, SEL_X2, SEL_Y2);
    }
    true
}

/// Отрисовка полноэкранного режима выделения области
pub unsafe fn draw_snip_overlay(fb: *mut u32, scr_w: isize, scr_h: isize, mx: isize, my: isize) {
    if !SNIP_SELECTING {
        return;
    }
    let src = SCREEN_SNAP.as_ptr();
    let lx = SEL_X1.min(SEL_X2);
    let rx = SEL_X1.max(SEL_X2);
    let ty = SEL_Y1.min(SEL_Y2);
    let by = SEL_Y1.max(SEL_Y2);

    let mut y = 0isize;
    while y < scr_h {
        let row_off = y * scr_w;
        let dst_row = fb.offset(row_off);
        let src_row = src.offset(row_off);
        let mut x = 0isize;
        while x < scr_w {
            let px = read_volatile(src_row.offset(x));
            let inside = SEL_DRAGGING && x >= lx && x <= rx && y >= ty && y <= by;
            let out_col = if inside { px } else { (px >> 1) & 0x007F7F7F };
            write_volatile(dst_row.offset(x), out_col);
            x += 1;
        }
        y += 1;
    }

    // Рамка вокруг выделения
    if SEL_DRAGGING && rx > lx && by > ty {
        fill_rect(fb, scr_w, lx, ty, rx - lx, 2, 0x0038BDF8);
        fill_rect(fb, scr_w, lx, by - 2, rx - lx, 2, 0x0038BDF8);
        fill_rect(fb, scr_w, lx, ty, 2, by - ty, 0x0038BDF8);
        fill_rect(fb, scr_w, rx - 2, ty, 2, by - ty, 0x0038BDF8);
    }

    // Верхняя подсказка
    fill_rect(fb, scr_w, scr_w / 2 - 190, 14, 380, 28, 0x000F172A);
    fill_rect(fb, scr_w, scr_w / 2 - 190, 14, 380, 1, 0x0038BDF8);
    crate::kernels::kernel_64::draw_txt(
        fb,
        scr_w / 2 - 172,
        23,
        "Зажмите ЛКМ и выделите область (ESC - отмена)",
                                        0x00F8FAFC,
    );

    // Перекрестие курсора
    fill_rect(fb, scr_w, mx - 8, my, 17, 1, 0x0038BDF8);
    fill_rect(fb, scr_w, mx, my - 8, 1, 17, 0x0038BDF8);
}

/// Отрисовка окна "Ножницы"
pub unsafe fn draw_snip_window(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_SNIP {
        return;
    }
    let wx = SNIP_X;
    let wy = SNIP_Y;
    let ww = SNIP_W;
    let wh = SNIP_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Ножницы — Скриншоты экрана", 0x0038BDF8);

    // [X]
    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    // Панель кнопок сверху
    let by = wy + 36;
    let b1_hov = mx >= wx + 14 && mx <= wx + 138 && my >= by && my <= by + 26;
    fill_rect(fb, stride, wx + 14, by, 124, 26, if b1_hov { 0x002563EB } else { 0x001D4ED8 });
    crate::kernels::kernel_64::draw_txt(fb, wx + 26, by + 8, "Выделить зону", 0x00FFFFFF);

    let b2_hov = mx >= wx + 146 && mx <= wx + 258 && my >= by && my <= by + 26;
    fill_rect(fb, stride, wx + 146, by, 112, 26, if b2_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 162, by + 8, "Весь экран", 0x00F8FAFC);

    let b3_hov = mx >= wx + 266 && mx <= wx + 386 && my >= by && my <= by + 26;
    fill_rect(fb, stride, wx + 266, by, 120, 26, if b3_hov { 0x0016A34A } else { 0x0015803D });
    crate::kernels::kernel_64::draw_txt(fb, wx + 278, by + 8, "Сохранить BMP", 0x00FFFFFF);

    // Область предпросмотра 360x180
    let px = wx + 20;
    let py = wy + 72;
    fill_rect(fb, stride, px - 1, py - 1, 362, 182, 0x00334155);

    if CAPTURED_W == 0 {
        fill_rect(fb, stride, px, py, 360, 180, 0x00020617);
        crate::kernels::kernel_64::draw_txt(fb, px + 68, py + 82, "Нажмите «Выделить зону» для снимка", 0x0094A3B8);
    } else {
        let src = CAPTURED_BUF.as_ptr();
        let mut dy = 0isize;
        while dy < 180 {
            let row = fb.offset((py + dy) * stride);
            let mut dx = 0isize;
            while dx < 360 {
                write_volatile(row.offset(px + dx), read_volatile(src.offset(dy * 360 + dx)));
                dx += 1;
            }
            dy += 1;
        }
    }

    if SAVED_NOTICE > 0 {
        SAVED_NOTICE -= 1;
        crate::kernels::kernel_64::draw_txt(fb, wx + 20, wy + wh - 24, "Сохранено в VFS: screen.bmp!", 0x004ADE80);
    } else {
        crate::kernels::kernel_64::draw_txt(fb, wx + 20, wy + wh - 24, "Формат: 24-bit BMP (VFS)", 0x0064748B);
    }
}

pub unsafe fn handle_snip_click(
    mx: isize,
    my: isize,
    src_fb: *const u32,
    scr_w: isize,
    scr_h: isize,
    drag_win: &mut u8,
    drag_off_x: &mut isize,
    drag_off_y: &mut isize,
    focused_win: &mut u8,
) -> bool {
    if !WIN_SNIP {
        return false;
    }
    let wx = SNIP_X;
    let wy = SNIP_Y;
    let ww = SNIP_W;
    let wh = SNIP_H;

    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        return false;
    }

    *focused_win = 12;

    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_SNIP = false;
        return true;
    }

    if my >= wy && my <= wy + 28 {
        *drag_win = 12;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }

    let by = wy + 36;
    if my >= by && my <= by + 26 {
        // Выделить зону
        if mx >= wx + 14 && mx <= wx + 138 {
            start_snip_selection(src_fb, scr_w, scr_h);
            return true;
        }
        // Весь экран
        if mx >= wx + 146 && mx <= wx + 258 {
            start_snip_selection(src_fb, scr_w, scr_h);
            finalize_capture(scr_w, scr_h, 0, 0, scr_w, scr_h);
            return true;
        }
        // Сохранить BMP в VFS
        if mx >= wx + 266 && mx <= wx + 386 && CAPTURED_W > 0 {
            crate::kernels::kernel_64::save_snip_to_vfs(CAPTURED_BUF.as_ptr(), 360, 180);
            SAVED_NOTICE = 180;
            return true;
        }
    }

    true
}
