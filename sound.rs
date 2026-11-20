#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

pub const PLAYER_W: isize = 340;
pub const PLAYER_H: isize = 260;

pub static mut WIN_PLAYER: bool = false;
pub static mut PLAYER_X: isize = 260;
pub static mut PLAYER_Y: isize = 120;

pub static mut CURRENT_TRACK: usize = 0;
pub static mut IS_PLAYING: bool = false;
static mut NOTE_IDX: usize = 0;
static mut NOTE_TICKS: u32 = 0;

const TRACK_BOOT: [(u32, u32); 6] = [
    (523, 6), (659, 6), (784, 6), (1046, 12), (784, 6), (1046, 16),
];

const TRACK_WIN: [(u32, u32); 8] = [
    (523, 5), (659, 5), (784, 5), (1046, 10), (0, 3), (784, 5), (1046, 16), (0, 2),
];

const TRACK_MARCH: [(u32, u32); 14] = [
    (440, 14), (440, 14), (440, 14), (349, 10), (523, 4),
    (440, 14), (349, 10), (523, 4), (440, 24), (0, 6),
    (659, 14), (659, 14), (659, 14), (698, 10),
];

const TRACK_TETRIS: [(u32, u32); 16] = [
    (659, 10), (494, 5), (523, 5), (587, 10), (523, 5), (494, 5),
    (440, 10), (440, 5), (523, 5), (659, 10), (587, 5), (523, 5),
    (494, 14), (523, 5), (587, 10), (659, 10),
];

unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
}

unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    val
}

pub unsafe fn play_freq(freq: u32) {
    if freq == 0 {
        stop_sound();
        return;
    }
    let div = 1_193_180u32 / freq;
    outb(0x43, 0xB6);
    outb(0x42, (div & 0xFF) as u8);
    outb(0x42, ((div >> 8) & 0xFF) as u8);
    let tmp = inb(0x61);
    if (tmp & 3) != 3 {
        outb(0x61, tmp | 3);
    }
}

pub unsafe fn stop_sound() {
    let tmp = inb(0x61) & 0xFC;
    outb(0x61, tmp);
}

pub unsafe fn start_track(track_id: usize, sound_enabled: bool) {
    if !sound_enabled {
        stop_sound();
        IS_PLAYING = false;
        return;
    }
    CURRENT_TRACK = track_id % 4;
    NOTE_IDX = 0;
    NOTE_TICKS = 0;
    IS_PLAYING = true;
}

pub unsafe fn tick_sound(sound_enabled: bool) {
    if !sound_enabled || !IS_PLAYING {
        if !IS_PLAYING {
            stop_sound();
        }
        return;
    }

    let (notes_ptr, len) = match CURRENT_TRACK {
        0 => (TRACK_BOOT.as_ptr(), TRACK_BOOT.len()),
        1 => (TRACK_WIN.as_ptr(), TRACK_WIN.len()),
        2 => (TRACK_MARCH.as_ptr(), TRACK_MARCH.len()),
        _ => (TRACK_TETRIS.as_ptr(), TRACK_TETRIS.len()),
    };

    if NOTE_IDX >= len {
        IS_PLAYING = false;
        stop_sound();
        return;
    }

    let (freq, dur) = read_volatile(notes_ptr.add(NOTE_IDX));
    if NOTE_TICKS == 0 {
        play_freq(freq);
    }
    NOTE_TICKS += 1;
    if NOTE_TICKS >= dur {
        NOTE_TICKS = 0;
        NOTE_IDX += 1;
    }
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

pub unsafe fn draw_player(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_PLAYER {
        return;
    }
    let wx = PLAYER_X;
    let wy = PLAYER_Y;
    let ww = PLAYER_W;
    let wh = PLAYER_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "midenPlayer — Синтезатор", 0x0038BDF8);

    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    let names = [
        "1. Приветствие midenOS",
        "2. Фанфары победы",
        "3. Имперский марш",
        "4. Мелодия Тетрис",
    ];
    let n_ptr = names.as_ptr();
    let mut i = 0isize;
    while i < 4 {
        let ty = wy + 40 + i * 34;
        let active = CURRENT_TRACK == (i as usize);
        let hov = mx >= wx + 14 && mx <= wx + ww - 14 && my >= ty && my <= ty + 28;
        let col = if active {
            0x000284C7
        } else if hov {
            0x00334155
        } else {
            0x001E293B
        };
        fill_rect(fb, stride, wx + 14, ty, ww - 28, 28, col);
        let label = read_volatile(n_ptr.add(i as usize));
        crate::kernels::kernel_64::draw_txt(fb, wx + 24, ty + 9, label, 0x00F8FAFC);
        i += 1;
    }

    let eq_y = wy + 186;
    let mut b = 0isize;
    while b < 12 {
        let bar_h = if IS_PLAYING {
            (((NOTE_TICKS as isize + b * 3) * 7) % 18) + 4
        } else {
            3
        };
        fill_rect(fb, stride, wx + 18 + b * 12, eq_y + (20 - bar_h), 8, bar_h, 0x004ADE80);
        b += 1;
    }

    let p_hov = mx >= wx + 175 && mx <= wx + 248 && my >= eq_y - 2 && my <= eq_y + 24;
    fill_rect(fb, stride, wx + 175, eq_y - 2, 73, 26, if p_hov { 0x0016A34A } else { 0x0015803D });
    crate::kernels::kernel_64::draw_txt(fb, wx + 188, eq_y + 6, "Играть", 0x00FFFFFF);

    let s_hov = mx >= wx + 254 && mx <= wx + 326 && my >= eq_y - 2 && my <= eq_y + 24;
    fill_rect(fb, stride, wx + 254, eq_y - 2, 72, 26, if s_hov { 0x00DC2626 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 272, eq_y + 6, "Стоп", 0x00FFFFFF);

    let st_msg = if IS_PLAYING { "Воспроизведение PC Speaker..." } else { "Готов к воспроизведению" };
    crate::kernels::kernel_64::draw_txt(fb, wx + 14, wy + wh - 20, st_msg, 0x0094A3B8);
}

pub unsafe fn handle_player_click(
    mx: isize,
    my: isize,
    sound_enabled: bool,
    drag_win: &mut u8,
    drag_off_x: &mut isize,
    drag_off_y: &mut isize,
    focused_win: &mut u8,
) -> bool {
    if !WIN_PLAYER {
        return false;
    }
    let wx = PLAYER_X;
    let wy = PLAYER_Y;
    let ww = PLAYER_W;
    let wh = PLAYER_H;

    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        return false;
    }

    *focused_win = 10;

    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_PLAYER = false;
        IS_PLAYING = false;
        stop_sound();
        return true;
    }

    if my >= wy && my <= wy + 28 {
        *drag_win = 10;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }

    let mut i = 0isize;
    while i < 4 {
        let ty = wy + 40 + i * 34;
        if mx >= wx + 14 && mx <= wx + ww - 14 && my >= ty && my <= ty + 28 {
            start_track(i as usize, sound_enabled);
            return true;
        }
        i += 1;
    }

    let eq_y = wy + 186;
    if mx >= wx + 175 && mx <= wx + 248 && my >= eq_y - 2 && my <= eq_y + 24 {
        start_track(CURRENT_TRACK, sound_enabled);
        return true;
    }
    if mx >= wx + 254 && mx <= wx + 326 && my >= eq_y - 2 && my <= eq_y + 24 {
        IS_PLAYING = false;
        stop_sound();
        return true;
    }

    true
}
