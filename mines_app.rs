#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

pub const MINES_W: isize = 310;
pub const MINES_H: isize = 360;

const COLS: usize = 10;
const ROWS: usize = 10;
const TOTAL_MINES: u8 = 14;

pub static mut WIN_MINES: bool = false;
pub static mut MINES_X: isize = 220;
pub static mut MINES_Y: isize = 90;

// Состояние поля: 0..8 = число мин вокруг, 9 = мина
static mut GRID: [u8; 100] = [0; 100];
// Видимость: 0 = закрыта, 1 = открыта, 2 = флажок
static mut STATE: [u8; 100] = [0; 100];

pub static mut GAME_OVER: bool = false;
pub static mut GAME_WON: bool = false;
pub static mut FLAGS_LEFT: isize = 14;
static mut RNG_SEED: u32 = 0x1337_BEEF;

unsafe fn next_rand() -> u32 {
    let mut x = RNG_SEED;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    RNG_SEED = x;
    x
}

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

unsafe fn draw_border(fb: *mut u32, stride: isize, x: isize, y: isize, w: isize, h: isize, color: u32) {
    fill_rect(fb, stride, x, y, w, 1, color);
    fill_rect(fb, stride, x, y + h - 1, w, 1, color);
    fill_rect(fb, stride, x, y, 1, h, color);
    fill_rect(fb, stride, x + w - 1, y, 1, h, color);
}

pub unsafe fn reset_mines(seed_extra: u32) {
    if seed_extra != 0 {
        RNG_SEED = RNG_SEED.wrapping_add(seed_extra).wrapping_mul(1664525).wrapping_add(1013904223);
    }
    let g_ptr = GRID.as_mut_ptr();
    let s_ptr = STATE.as_mut_ptr();
    let mut i = 0;
    while i < 100 {
        write_volatile(g_ptr.add(i), 0);
        write_volatile(s_ptr.add(i), 0);
        i += 1;
    }

    GAME_OVER = false;
    GAME_WON = false;
    FLAGS_LEFT = TOTAL_MINES as isize;

    // Расстановка мин
    let mut placed = 0u8;
    while placed < TOTAL_MINES {
        let idx = (next_rand() as usize) % 100;
        if read_volatile(g_ptr.add(idx)) != 9 {
            write_volatile(g_ptr.add(idx), 9);
            placed += 1;
        }
    }

    // Подсчёт соседей
    let mut r = 0isize;
    while r < 10 {
        let mut c = 0isize;
        while c < 10 {
            let idx = (r * 10 + c) as usize;
            if read_volatile(g_ptr.add(idx)) != 9 {
                let mut count = 0u8;
                let mut dr = -1isize;
                while dr <= 1 {
                    let mut dc = -1isize;
                    while dc <= 1 {
                        let nr = r + dr;
                        let nc = c + dc;
                        if nr >= 0 && nr < 10 && nc >= 0 && nc < 10 {
                            let nidx = (nr * 10 + nc) as usize;
                            if read_volatile(g_ptr.add(nidx)) == 9 {
                                count += 1;
                            }
                        }
                        dc += 1;
                    }
                    dr += 1;
                }
                write_volatile(g_ptr.add(idx), count);
            }
            c += 1;
        }
        r += 1;
    }
}

unsafe fn flood_reveal(start_r: isize, start_c: isize) {
    let mut stack = [0u8; 100];
    let stk_ptr = stack.as_mut_ptr();
    let mut top = 0usize;

    write_volatile(stk_ptr.add(top), (start_r * 10 + start_c) as u8);
    top += 1;

    let g_ptr = GRID.as_ptr();
    let s_ptr = STATE.as_mut_ptr();

    while top > 0 {
        top -= 1;
        let idx = read_volatile(stk_ptr.add(top)) as usize;
        if read_volatile(s_ptr.add(idx)) != 0 {
            continue;
        }
        write_volatile(s_ptr.add(idx), 1);

        if read_volatile(g_ptr.add(idx)) == 0 {
            let r = (idx / 10) as isize;
            let c = (idx % 10) as isize;
            let mut dr = -1isize;
            while dr <= 1 {
                let mut dc = -1isize;
                while dc <= 1 {
                    let nr = r + dr;
                    let nc = c + dc;
                    if nr >= 0 && nr < 10 && nc >= 0 && nc < 10 {
                        let nidx = (nr * 10 + nc) as usize;
                        if read_volatile(s_ptr.add(nidx)) == 0 && top < 100 {
                            write_volatile(stk_ptr.add(top), nidx as u8);
                            top += 1;
                        }
                    }
                    dc += 1;
                }
                dr += 1;
            }
        }
    }
}

unsafe fn check_win() {
    let g_ptr = GRID.as_ptr();
    let s_ptr = STATE.as_ptr();
    let mut i = 0;
    while i < 100 {
        if read_volatile(g_ptr.add(i)) != 9 && read_volatile(s_ptr.add(i)) != 1 {
            return;
        }
        i += 1;
    }
    GAME_WON = true;
}

pub unsafe fn draw_mines(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_MINES {
        return;
    }
    let wx = MINES_X;
    let wy = MINES_Y;
    let ww = MINES_W;
    let wh = MINES_H;

    // Корпус окна
    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    draw_border(fb, stride, wx, wy, ww, wh, if focused { 0x0038BDF8 } else { 0x00334155 });

    // Шапка
    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "midenMines — Сапёр", 0x0038BDF8);

    // Кнопка закрытия [X]
    let close_x = wx + ww - 28;
    let close_y = wy + 4;
    let c_hov = mx >= close_x && mx <= close_x + 22 && my >= close_y && my <= close_y + 20;
    fill_rect(fb, stride, close_x, close_y, 22, 20, if c_hov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, close_x + 7, close_y + 6, "X", 0x00FFFFFF);

    // Верхняя панель статуса и кнопка "Новая игра"
    let status_txt = if GAME_WON {
        "ПОБЕДА! Все мины найдены"
    } else if GAME_OVER {
        "ВЗРЫВ! Игра окончена"
    } else {
        "ЛКМ: открыть | ПКМ: флаг"
    };
    let status_col = if GAME_WON {
        0x004ADE80
    } else if GAME_OVER {
        0x00EF4444
    } else {
        0x0094A3B8
    };
    crate::kernels::kernel_64::draw_txt(fb, wx + 14, wy + 38, status_txt, status_col);

    // Кнопка "Заново"
    let rst_x = wx + ww - 84;
    let rst_y = wy + 34;
    let r_hov = mx >= rst_x && mx <= rst_x + 70 && my >= rst_y && my <= rst_y + 22;
    fill_rect(fb, stride, rst_x, rst_y, 70, 22, if r_hov { 0x002563EB } else { 0x001D4ED8 });
    crate::kernels::kernel_64::draw_txt(fb, rst_x + 12, rst_y + 6, "Заново", 0x00FFFFFF);

    // Игровое поле 10x10 (размер клетки 26x26 + отступ 2px = 28px)
    let start_x = wx + 15;
    let start_y = wy + 66;
    let g_ptr = GRID.as_ptr();
    let s_ptr = STATE.as_ptr();

    let mut r = 0isize;
    while r < 10 {
        let mut c = 0isize;
        while c < 10 {
            let idx = (r * 10 + c) as usize;
            let cx = start_x + c * 28;
            let cy = start_y + r * 28;
            let st = read_volatile(s_ptr.add(idx));
            let val = read_volatile(g_ptr.add(idx));

            let hov = mx >= cx && mx <= cx + 26 && my >= cy && my <= cy + 26;

            if st == 0 {
                // Закрытая клетка
                fill_rect(fb, stride, cx, cy, 26, 26, if hov { 0x00334155 } else { 0x001E293B });
                fill_rect(fb, stride, cx, cy, 26, 1, 0x00475569);
            } else if st == 2 {
                // Флажок
                fill_rect(fb, stride, cx, cy, 26, 26, 0x001E293B);
                fill_rect(fb, stride, cx + 8, cy + 6, 10, 8, 0x00EF4444);
                fill_rect(fb, stride, cx + 8, cy + 6, 2, 15, 0x00F8FAFC);
            } else {
                // Открытая клетка
                if val == 9 {
                    fill_rect(fb, stride, cx, cy, 26, 26, 0x00991B1B);
                    crate::kernels::kernel_64::draw_txt(fb, cx + 9, cy + 9, "*", 0x00FFFFFF);
                } else {
                    fill_rect(fb, stride, cx, cy, 26, 26, 0x00090D16);
                    if val > 0 {
                        let (num_str, num_col) = match val {
                            1 => ("1", 0x0038BDF8),
                            2 => ("2", 0x004ADE80),
                            3 => ("3", 0x00F87171),
                            4 => ("4", 0x00C084FC),
                            _ => ("5", 0x00FACC15),
                        };
                        crate::kernels::kernel_64::draw_txt(fb, cx + 9, cy + 9, num_str, num_col);
                    }
                }
            }
            c += 1;
        }
        r += 1;
    }
}

pub unsafe fn handle_mines_click(
    mx: isize,
    my: isize,
    is_right_click: bool,
    drag_win: &mut u8,
    drag_off_x: &mut isize,
    drag_off_y: &mut isize,
    focused_win: &mut u8,
) -> bool {
    if !WIN_MINES {
        return false;
    }
    let wx = MINES_X;
    let wy = MINES_Y;
    let ww = MINES_W;
    let wh = MINES_H;

    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        return false;
    }

    *focused_win = 9;

    // Закрытие [X]
    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_MINES = false;
        return true;
    }

    // Перетаскивание за шапку
    if my >= wy && my <= wy + 28 {
        *drag_win = 9;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }

    // Кнопка "Заново"
    let rst_x = wx + ww - 84;
    let rst_y = wy + 34;
    if mx >= rst_x && mx <= rst_x + 70 && my >= rst_y && my <= rst_y + 22 {
        reset_mines((mx as u32).wrapping_mul(31) ^ (my as u32));
        return true;
    }

    if GAME_OVER || GAME_WON {
        return true;
    }
    // Клик по игровому полю 10x10
    let start_x = wx + 15;
    let start_y = wy + 66;
    if mx >= start_x && mx < start_x + 280 && my >= start_y && my < start_y + 280 {
        let c = (mx - start_x) / 28;
        let r = (my - start_y) / 28;
        let idx = (r * 10 + c) as usize;
        let s_ptr = STATE.as_mut_ptr();
        let g_ptr = GRID.as_ptr();
        let cur_st = read_volatile(s_ptr.add(idx));
        if is_right_click {
            if cur_st == 0 {
                write_volatile(s_ptr.add(idx), 2);
                FLAGS_LEFT -= 1;
            } else if cur_st == 2 {
                write_volatile(s_ptr.add(idx), 0);
                FLAGS_LEFT += 1;
            }
        } else if cur_st == 0 {
            if read_volatile(g_ptr.add(idx)) == 9 {
                GAME_OVER = true;
                // Открываем все мины
                let mut i = 0;
                while i < 100 {
                    if read_volatile(g_ptr.add(i)) == 9 {
                        write_volatile(s_ptr.add(i), 1);
                    }
                    i += 1;
                }
            } else {
                flood_reveal(r, c);
                check_win();
            }
        }
    }
    true
}
