#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

pub const TODO_W: isize = 360;
pub const TODO_H: isize = 310;

pub static mut WIN_TODO: bool = false;
pub static mut TODO_X: isize = 290;
pub static mut TODO_Y: isize = 100;

static mut TASK_DONE: [bool; 6] = [true, false, false, false, false, false];
static mut TASK_ACTIVE: [bool; 6] = [true, true, true, true, false, false];
static mut PRESET_NEXT: usize = 0;
static mut SAVE_MSG_TICKS: u32 = 0;

const TASKS_TEXT: [&str; 6] = [
    "Запустить midenOS на реальном ПК",
"Протестировать экран сбоя RDM",
"Сделать скриншот в Ножницах",
"Проверить чтение CD/DVD привода",
"Написать скрипт на своём языке",
"Создать резервный архив в VFS",
];

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

pub unsafe fn save_todo_to_vfs() {
    let mut buf = [0u8; 512];
    let dst = buf.as_mut_ptr();
    let mut pos = 0usize;

    let mut i = 0usize;
    while i < 6 {
        if read_volatile(TASK_ACTIVE.as_ptr().add(i)) {
            let done = read_volatile(TASK_DONE.as_ptr().add(i));
            let prefix = if done { b"[x] " } else { b"[ ] " };
            let mut k = 0;
            while k < 4 && pos < 500 {
                write_volatile(dst.add(pos), prefix[k]);
                pos += 1;
                k += 1;
            }
            let tb = TASKS_TEXT[i].as_bytes();
            let mut j = 0;
            while j < tb.len() && pos < 500 {
                write_volatile(dst.add(pos), tb[j]);
                pos += 1;
                j += 1;
            }
            write_volatile(dst.add(pos), 10);
            pos += 1;
        }
        i += 1;
    }

    crate::kernels::kernel_64::write_vfs_file("todo.txt", &buf[..pos]);
    SAVE_MSG_TICKS = 150;
}

pub unsafe fn draw_todo_window(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_TODO { return; }
    let wx = TODO_X;
    let wy = TODO_Y;
    let ww = TODO_W;
    let wh = TODO_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Задачи (To-Do) — todo.txt", 0x0038BDF8);

    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    let mut i = 0isize;
    while i < 6 {
        let ry = wy + 38 + i * 34;
        let act = read_volatile(TASK_ACTIVE.as_ptr().add(i as usize));
        let done = read_volatile(TASK_DONE.as_ptr().add(i as usize));
        fill_rect(fb, stride, wx + 14, ry, ww - 28, 28, 0x001E293B);

        if act {
            fill_rect(fb, stride, wx + 22, ry + 6, 16, 16, if done { 0x0016A34A } else { 0x00020617 });
            if done {
                crate::kernels::kernel_64::draw_txt(fb, wx + 26, ry + 8, "v", 0x00FFFFFF);
            }
            let col = if done { 0x0064748B } else { 0x00F8FAFC };
            crate::kernels::kernel_64::draw_txt(fb, wx + 46, ry + 8, TASKS_TEXT[i as usize], col);
        } else {
            crate::kernels::kernel_64::draw_txt(fb, wx + 24, ry + 8, "(Пустой слот задачи)", 0x00475569);
        }
        i += 1;
    }

    let by = wy + wh - 42;
    fill_rect(fb, stride, wx + 14, by, 148, 28, 0x001E293B);
    crate::kernels::kernel_64::draw_txt(fb, wx + 28, by + 9, "+ Добавить задачу", 0x0038BDF8);

    fill_rect(fb, stride, wx + 172, by, 174, 28, 0x0015803D);
    let stxt = if SAVE_MSG_TICKS > 0 {
        SAVE_MSG_TICKS -= 1;
        "Сохранено в todo.txt!"
    } else {
        "Сохранить в VFS"
    };
    crate::kernels::kernel_64::draw_txt(fb, wx + 190, by + 9, stxt, 0x00FFFFFF);
}

pub unsafe fn handle_todo_click(
    mx: isize, my: isize,
    drag_win: &mut u8, drag_off_x: &mut isize, drag_off_y: &mut isize, focused_win: &mut u8,
) -> bool {
    if !WIN_TODO { return false; }
    let wx = TODO_X;
    let wy = TODO_Y;
    let ww = TODO_W;
    let wh = TODO_H;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh { return false; }

    *focused_win = 16;
    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_TODO = false;
        return true;
    }
    if my >= wy && my <= wy + 28 {
        *drag_win = 16;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }

    let mut i = 0isize;
    while i < 6 {
        let ry = wy + 38 + i * 34;
        if mx >= wx + 14 && mx <= wx + ww - 14 && my >= ry && my <= ry + 28 {
            if read_volatile(TASK_ACTIVE.as_ptr().add(i as usize)) {
                let p = TASK_DONE.as_mut_ptr().add(i as usize);
                write_volatile(p, !read_volatile(p));
            }
            return true;
        }
        i += 1;
    }

    let by = wy + wh - 42;
    if my >= by && my <= by + 28 {
        if mx >= wx + 14 && mx <= wx + 162 {
            let mut k = 0usize;
            while k < 6 {
                let ap = TASK_ACTIVE.as_mut_ptr().add(k);
                if !read_volatile(ap) {
                    write_volatile(ap, true);
                    write_volatile(TASK_DONE.as_mut_ptr().add(k), false);
                    break;
                }
                k += 1;
            }
            PRESET_NEXT = (PRESET_NEXT + 1) % 6;
            return true;
        }
        if mx >= wx + 172 && mx <= wx + 346 {
            save_todo_to_vfs();
            return true;
        }
    }

    true
}
