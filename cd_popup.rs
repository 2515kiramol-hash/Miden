#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};
use crate::disk::{OpticalTrayState, TRAY_STATE, LAST_DISC_LABEL, LAST_DISC_LABEL_LEN};

pub static mut WIN_CD_POPUP: bool = false;
pub static mut CD_X: isize = 310;
pub static mut CD_Y: isize = 180;
pub const CD_W: isize = 380;
pub const CD_H: isize = 250;

static mut FILES_BUF: [u8; 1024] = [0; 1024];
static mut FILES_LEN: usize = 0;
static mut READ_DELAY_TICKS: u32 = 0;

unsafe fn fill_rect(fb: *mut u32, stride: isize, x: isize, y: isize, w: isize, h: isize, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let mut dy = 0;
    while dy < h {
        let py = y + dy;
        if py >= 0 && py < 768 {
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

/// Вызывается в главном цикле ядра для автоматического отслеживания дисковода
pub unsafe fn tick_cd_monitor(tick: u32) {
    if READ_DELAY_TICKS > 0 {
        READ_DELAY_TICKS -= 1;
        if READ_DELAY_TICKS == 0 {
            FILES_LEN = crate::disk::list_cd_files(&mut FILES_BUF);
        }
        return;
    }

    if tick % 45 != 0 {
        return;
    }

    let prev = TRAY_STATE;
    let cur = crate::disk::poll_optical_drive();

    // Если лоток открыли — открываем окно с надписью "Вставьте диск"
    if cur == OpticalTrayState::TrayOpenOrEmpty && prev != OpticalTrayState::TrayOpenOrEmpty {
        WIN_CD_POPUP = true;
        FILES_LEN = 0;
    }

    // Если диск только что вставили — открываем окно и запускаем чтение
    if cur == OpticalTrayState::MediaJustInserted {
        WIN_CD_POPUP = true;
        FILES_LEN = 0;
        READ_DELAY_TICKS = 35; // Короткая анимация "Чтение диска..." перед выводом файлов
    }
}

pub unsafe fn draw_cd_popup(fb: *mut u32, stride: isize, mx: isize, my: isize) {
    if !WIN_CD_POPUP {
        return;
    }
    let wx = CD_X;
    let wy = CD_Y;
    let ww = CD_W;
    let wh = CD_H;

    // Корпус окна
    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, 0x0038BDF8);

    // Шапка
    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, 0x001E293B);
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Оптический привод (CD/DVD)", 0x0038BDF8);

    // Кнопка [X]
    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    // Состояние диска
    if TRAY_STATE == OpticalTrayState::NoDrive {
        crate::kernels::kernel_64::draw_txt(fb, wx + 16, wy + 46, "CD/DVD-привод не обнаружен на шине IDE", 0x00EF4444);
    } else if TRAY_STATE == OpticalTrayState::TrayOpenOrEmpty {
        fill_rect(fb, stride, wx + 16, wy + 42, ww - 32, 46, 0x001E293B);
        crate::kernels::kernel_64::draw_txt(fb, wx + 28, wy + 52, "Лоток открыт или пуст", 0x00FACC15);
        crate::kernels::kernel_64::draw_txt(fb, wx + 28, wy + 70, "Вставьте диск в дисковод...", 0x00F8FAFC);
    } else if READ_DELAY_TICKS > 0 {
        fill_rect(fb, stride, wx + 16, wy + 42, ww - 32, 46, 0x001E293B);
        crate::kernels::kernel_64::draw_txt(fb, wx + 28, wy + 52, "Диск обнаружен!", 0x004ADE80);
        crate::kernels::kernel_64::draw_txt(fb, wx + 28, wy + 70, "Чтение файловой системы ISO9660...", 0x0038BDF8);
    } else {
        // Диск прочитан — показываем метку тома и список файлов
        crate::kernels::kernel_64::draw_txt(fb, wx + 16, wy + 38, "Диск готов • Том:", 0x004ADE80);
        if LAST_DISC_LABEL_LEN > 0 {
            let lbl_slice = core::slice::from_raw_parts(LAST_DISC_LABEL.as_ptr(), LAST_DISC_LABEL_LEN);
            if let Ok(lbl) = core::str::from_utf8(lbl_slice) {
                crate::kernels::kernel_64::draw_txt(fb, wx + 156, wy + 38, lbl, 0x00F8FAFC);
            }
        } else {
            crate::kernels::kernel_64::draw_txt(fb, wx + 156, wy + 38, "CDROM_DISC", 0x00F8FAFC);
        }

        // Рамка списка файлов
        fill_rect(fb, stride, wx + 16, wy + 58, ww - 32, 142, 0x00020617);
        if FILES_LEN == 0 {
            crate::kernels::kernel_64::draw_txt(fb, wx + 26, wy + 70, "(Корневой каталог пуст или не ISO9660)", 0x0094A3B8);
        } else {
            let f_ptr = FILES_BUF.as_ptr();
            let mut line_y = wy + 66;
            let mut start = 0usize;
            let mut idx = 0usize;
            while idx <= FILES_LEN && line_y < wy + 188 {
                if idx == FILES_LEN || read_volatile(f_ptr.add(idx)) == 10 {
                    let len = idx - start;
                    if len > 0 {
                        let s = core::slice::from_raw_parts(f_ptr.add(start), len);
                        if let Ok(fname) = core::str::from_utf8(s) {
                            crate::kernels::kernel_64::draw_txt(fb, wx + 26, line_y, fname, 0x00E2E8F0);
                        }
                        line_y += 16;
                    }
                    start = idx + 1;
                }
                idx += 1;
            }
        }
    }

    // Нижние кнопки: [Открыть лоток] и [Обновить]
    let ej_hov = mx >= wx + 16 && mx <= wx + 156 && my >= wy + wh - 36 && my <= wy + wh - 10;
    fill_rect(fb, stride, wx + 16, wy + wh - 36, 140, 26, if ej_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 32, wy + wh - 28, "Открыть лоток", 0x00F8FAFC);

    let rf_hov = mx >= wx + 166 && mx <= wx + 286 && my >= wy + wh - 36 && my <= wy + wh - 10;
    fill_rect(fb, stride, wx + 166, wy + wh - 36, 120, 26, if rf_hov { 0x002563EB } else { 0x001D4ED8 });
    crate::kernels::kernel_64::draw_txt(fb, wx + 188, wy + wh - 28, "Прочитать", 0x00FFFFFF);
}

pub unsafe fn handle_cd_popup_click(mx: isize, my: isize) -> bool {
    if !WIN_CD_POPUP {
        return false;
    }
    let wx = CD_X;
    let wy = CD_Y;
    let ww = CD_W;
    let wh = CD_H;

    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        return false;
    }

    // [X]
    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_CD_POPUP = false;
        return true;
    }

    // Кнопка "Открыть лоток"
    if mx >= wx + 16 && mx <= wx + 156 && my >= wy + wh - 36 && my <= wy + wh - 10 {
        let _ = crate::disk::eject_optical_tray(true);
        FILES_LEN = 0;
        return true;
    }

    // Кнопка "Прочитать"
    if mx >= wx + 166 && mx <= wx + 286 && my >= wy + wh - 36 && my <= wy + wh - 10 {
        let _ = crate::disk::poll_optical_drive();
        if TRAY_STATE == OpticalTrayState::MediaReady || TRAY_STATE == OpticalTrayState::MediaJustInserted {
            FILES_LEN = 0;
            READ_DELAY_TICKS = 20;
        }
        return true;
    }

    true
}
