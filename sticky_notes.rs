#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

#[derive(Copy, Clone)]
pub struct StickyNote {
    pub active: bool,
    pub x: isize,
    pub y: isize,
    pub color_idx: usize,
    pub text_idx: usize,
}

impl StickyNote {
    const fn empty() -> Self {
        Self { active: false, x: 560, y: 80, color_idx: 0, text_idx: 0 }
    }
}

pub static mut NOTES: [StickyNote; 4] = [
    StickyNote { active: false, x: 580, y: 74, color_idx: 0, text_idx: 0 },
StickyNote { active: false, x: 610, y: 210, color_idx: 1, text_idx: 1 },
StickyNote::empty(),
StickyNote::empty(),
];

const NOTE_TEXTS: [(&str, &str); 4] = [
    ("Идея для midenOS:", "Добавить свой язык кода"),
    ("Горячие клавиши:", "Ctrl+T Терминал | F11 Скрыть"),
    ("Напоминание:", "Проверить RDM и CD-привод"),
    ("Заметка:", "Все файлы хранятся в VFS"),
];

pub unsafe fn spawn_sticky_note() {
    let ptr = NOTES.as_mut_ptr();
    let mut i = 0usize;
    while i < 4 {
        let n = &mut *ptr.add(i);
        if !n.active {
            n.active = true;
            n.x = 540 + (i as isize) * 24;
            n.y = 70 + (i as isize) * 32;
            n.color_idx = i % 4;
            n.text_idx = i % 4;
            return;
        }
        i += 1;
    }
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

pub unsafe fn draw_sticky_notes(fb: *mut u32, stride: isize) {
    let ptr = NOTES.as_ptr();
    let mut i = 0usize;
    while i < 4 {
        let n = read_volatile(ptr.add(i));
        if n.active {
            let (hdr_col, body_col) = match n.color_idx % 4 {
                1 => (0x000284C7, 0x000C4A6E), // Голубой
                2 => (0x00DB2777, 0x00831843), // Розовый
                3 => (0x0016A34A, 0x0014532D), // Зелёный
                _ => (0x00CA8A04, 0x00713F12), // Жёлтый
            };
            let w = 220isize;
            let h = 104isize;
            fill_rect(fb, stride, n.x + 4, n.y + 4, w, h, 0x00020617);
            fill_rect(fb, stride, n.x, n.y, w, h, body_col);
            fill_rect(fb, stride, n.x, n.y, w, 22, hdr_col);

            crate::kernels::kernel_64::draw_txt(fb, n.x + 8, n.y + 6, "Стикер", 0x00FFFFFF);
            crate::kernels::kernel_64::draw_txt(fb, n.x + w - 42, n.y + 6, "[C]", 0x00FFFFFF);
            crate::kernels::kernel_64::draw_txt(fb, n.x + w - 18, n.y + 6, "X", 0x00FFFFFF);

            let (l1, l2) = NOTE_TEXTS[n.text_idx % 4];
            crate::kernels::kernel_64::draw_txt(fb, n.x + 10, n.y + 34, l1, 0x00F8FAFC);
            crate::kernels::kernel_64::draw_txt(fb, n.x + 10, n.y + 56, l2, 0x00E2E8F0);
            crate::kernels::kernel_64::draw_txt(fb, n.x + 10, n.y + 82, "[Клик: сменить текст]", 0x00CBD5E1);
        }
        i += 1;
    }
}

pub unsafe fn handle_sticky_click(
    mx: isize, my: isize,
    drag_win: &mut u8, drag_off_x: &mut isize, drag_off_y: &mut isize,
) -> bool {
    let ptr = NOTES.as_mut_ptr();
    let mut i = 4usize;
    while i > 0 {
        i -= 1;
        let n = &mut *ptr.add(i);
        if !n.active { continue; }
        let w = 220isize;
        let h = 104isize;
        if mx >= n.x && mx <= n.x + w && my >= n.y && my <= n.y + h {
            if my <= n.y + 22 {
                if mx >= n.x + w - 22 {
                    n.active = false;
                    return true;
                }
                if mx >= n.x + w - 46 {
                    n.color_idx = (n.color_idx + 1) % 4;
                    return true;
                }
                *drag_win = 20 + (i as u8);
                *drag_off_x = mx - n.x;
                *drag_off_y = my - n.y;
                return true;
            }
            n.text_idx = (n.text_idx + 1) % 4;
            return true;
        }
    }
    false
}
