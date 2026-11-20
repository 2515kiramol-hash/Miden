#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

pub const ARCH_W: isize = 380;
pub const ARCH_H: isize = 260;

pub static mut WIN_ARCHIVE: bool = false;
pub static mut ARCH_X: isize = 250;
pub static mut ARCH_Y: isize = 140;

static mut STATUS_TEXT: usize = 0; // 0 = готов, 1 = упакован backup.tar, 2 = распакован backup.tar, 3 = архив не найден

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

pub unsafe fn draw_archive_window(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_ARCHIVE { return; }
    let wx = ARCH_X;
    let wy = ARCH_Y;
    let ww = ARCH_W;
    let wh = ARCH_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Архиватор midenTAR (.tar)", 0x0038BDF8);

    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    fill_rect(fb, stride, wx + 16, wy + 40, ww - 32, 74, 0x001E293B);
    crate::kernels::kernel_64::draw_txt(fb, wx + 26, wy + 52, "Формат: POSIX USTAR (.tar)", 0x00F8FAFC);
    crate::kernels::kernel_64::draw_txt(fb, wx + 26, wy + 72, "Целевой архив: backup.tar (VFS)", 0x0038BDF8);
    crate::kernels::kernel_64::draw_txt(fb, wx + 26, wy + 92, "Упаковка и извлечение файлов VFS", 0x0094A3B8);

    // Кнопка "Создать backup.tar"
    let b1_hov = mx >= wx + 16 && mx <= wx + 184 && my >= wy + 130 && my <= wy + 164;
    fill_rect(fb, stride, wx + 16, wy + 130, 168, 34, if b1_hov { 0x002563EB } else { 0x001D4ED8 });
    crate::kernels::kernel_64::draw_txt(fb, wx + 32, wy + 142, "Создать backup.tar", 0x00FFFFFF);

    // Кнопка "Распаковать .tar"
    let b2_hov = mx >= wx + 196 && mx <= wx + 364 && my >= wy + 130 && my <= wy + 164;
    fill_rect(fb, stride, wx + 196, wy + 130, 168, 34, if b2_hov { 0x0016A34A } else { 0x0015803D });
    crate::kernels::kernel_64::draw_txt(fb, wx + 214, wy + 142, "Распаковать архив", 0x00FFFFFF);

    // Статус операции
    fill_rect(fb, stride, wx + 16, wy + 180, ww - 32, 56, 0x00020617);
    let (st_msg, st_col) = match STATUS_TEXT {
        1 => ("Готово: файлы VFS упакованы в backup.tar!", 0x004ADE80),
        2 => ("Готово: файлы из backup.tar извлечены в VFS!", 0x004ADE80),
        3 => ("Ошибка: файл backup.tar не найден в VFS.", 0x00EF4444),
        _ => ("Статус: выберите действие выше", 0x0094A3B8),
    };
    crate::kernels::kernel_64::draw_txt(fb, wx + 26, wy + 202, st_msg, st_col);
}

pub unsafe fn handle_archive_click(
    mx: isize, my: isize,
    drag_win: &mut u8, drag_off_x: &mut isize, drag_off_y: &mut isize, focused_win: &mut u8,
) -> bool {
    if !WIN_ARCHIVE { return false; }
    let wx = ARCH_X;
    let wy = ARCH_Y;
    let ww = ARCH_W;
    let wh = ARCH_H;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh { return false; }

    *focused_win = 18;
    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_ARCHIVE = false;
        return true;
    }
    if my >= wy && my <= wy + 28 {
        *drag_win = 18;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }

    if my >= wy + 130 && my <= wy + 164 {
        if mx >= wx + 16 && mx <= wx + 184 {
            crate::kernels::kernel_64::pack_vfs_to_tar();
            STATUS_TEXT = 1;
            return true;
        }
        if mx >= wx + 196 && mx <= wx + 364 {
            if crate::kernels::kernel_64::unpack_tar_from_vfs() {
                STATUS_TEXT = 2;
            } else {
                STATUS_TEXT = 3;
            }
            return true;
        }
    }

    true
}
