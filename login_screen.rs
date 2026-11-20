#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

static mut LOGIN_BG_CACHE: [u32; 1024 * 768] = [0; 1024 * 768];
static mut LOGIN_FRAME_BUF: [u32; 1024 * 768] = [0; 1024 * 768];

const LOGIN_WALL_1: &[u8] = include_bytes!("login_wall.bmp");
const LOGIN_WALL_2: &[u8] = include_bytes!("login_wall2.bmp");
const LOGIN_WALL_3: &[u8] = &[];

static mut SAVED_USER: [u8; 32] = *b"Administrator\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
static mut SAVED_USER_LEN: usize = 13;

static mut SAVED_PASS: [u8; 32] = [0; 32];
static mut SAVED_PASS_LEN: usize = 0;

static mut WALL_IDX: usize = 0;
static mut TZ_OFFSET: i8 = 0;
static mut CLOCK_OFF_X: isize = 0;
static mut CLOCK_OFF_Y: isize = 0;
static mut CARD_OFF_X: isize = 0;
static mut CARD_OFF_Y: isize = 0;
static mut LOGIN_CFG_OPEN: bool = false;

pub unsafe fn set_credentials(user: &[u8], pass: &[u8]) {
    let u_dst = SAVED_USER.as_mut_ptr();
    let u_len = user.len().min(31);
    let mut i = 0;
    while i < 32 {
        write_volatile(u_dst.add(i), 0);
        i += 1;
    }
    let mut j = 0;
    while j < u_len {
        write_volatile(u_dst.add(j), *user.as_ptr().add(j));
        j += 1;
    }
    SAVED_USER_LEN = if u_len > 0 { u_len } else { 13 };

    let p_dst = SAVED_PASS.as_mut_ptr();
    let p_len = pass.len().min(31);
    let mut k = 0;
    while k < 32 {
        write_volatile(p_dst.add(k), 0);
        k += 1;
    }
    let mut m = 0;
    while m < p_len {
        write_volatile(p_dst.add(m), *pass.as_ptr().add(m));
        m += 1;
    }
    SAVED_PASS_LEN = p_len;
}

pub unsafe fn get_saved_username() -> &'static str {
    let slice = core::slice::from_raw_parts(SAVED_USER.as_ptr(), SAVED_USER_LEN);
    core::str::from_utf8(slice).unwrap_or("Administrator")
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

unsafe fn read_cmos(reg: u8) -> u8 {
    outb(0x70, reg);
    let raw = inb(0x71);
    ((raw >> 4) * 10) + (raw & 0x0F)
}

unsafe fn get_rtc_datetime() -> (u8, u8, u8, u8, u8) {
    let raw_hr = read_cmos(0x04);
    let min = read_cmos(0x02);
    let day = read_cmos(0x07);
    let mon = read_cmos(0x08);
    let yr = read_cmos(0x09);
    let adj_hr = ((raw_hr as i16 + (TZ_OFFSET as i16)).rem_euclid(24)) as u8;
    (adj_hr, min, day, mon, yr)
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

unsafe fn blend_glass_rect(fb: *mut u32, stride: isize, max_h: isize, x: isize, y: isize, w: isize, h: isize, tint: u32, alpha: u32) {
    let inv = 256 - alpha;
    let tr = ((tint >> 16) & 0xFF) * alpha;
    let tg = ((tint >> 8) & 0xFF) * alpha;
    let tb = (tint & 0xFF) * alpha;

    let mut dy = 0;
    while dy < h {
        let py = y + dy;
        if py >= 0 && py < max_h {
            let row = fb.offset(py * stride);
            let mut dx = 0;
            while dx < w {
                let px = x + dx;
                if px >= 0 && px < stride {
                    let bg = read_volatile(row.offset(px));
                    let r = (((bg >> 16) & 0xFF) * inv + tr) >> 8;
                    let g = (((bg >> 8) & 0xFF) * inv + tg) >> 8;
                    let b = ((bg & 0xFF) * inv + tb) >> 8;
                    write_volatile(row.offset(px), (r << 16) | (g << 8) | b);
                }
                dx += 1;
            }
        }
        dy += 1;
    }
}

pub unsafe fn draw_bmp_fullscreen(fb: *mut u32, scr_w: isize, scr_h: isize, bmp: &[u8]) -> bool {
    if bmp.len() < 54 {
        return false;
    }
    let p = bmp.as_ptr();
    if read_volatile(p.add(0)) != b'B' || read_volatile(p.add(1)) != b'M' {
        return false;
    }

    let data_off = (read_volatile(p.add(10)) as usize)
        | ((read_volatile(p.add(11)) as usize) << 8)
        | ((read_volatile(p.add(12)) as usize) << 16)
        | ((read_volatile(p.add(13)) as usize) << 24);

    let img_w = (read_volatile(p.add(18)) as isize)
        | ((read_volatile(p.add(19)) as isize) << 8)
        | ((read_volatile(p.add(20)) as isize) << 16)
        | ((read_volatile(p.add(21)) as isize) << 24);

    let raw_h = (read_volatile(p.add(22)) as i32)
        | ((read_volatile(p.add(23)) as i32) << 8)
        | ((read_volatile(p.add(24)) as i32) << 16)
        | ((read_volatile(p.add(25)) as i32) << 24);

    let top_down = raw_h < 0;
    let img_h = (if top_down { -raw_h } else { raw_h }) as isize;
    let bpp = (read_volatile(p.add(28)) as usize) | ((read_volatile(p.add(29)) as usize) << 8);

    if img_w <= 0 || img_h <= 0 || (bpp != 24 && bpp != 32) {
        return false;
    }

    let bytes_per_px = bpp / 8;
    let row_stride = (img_w as usize * bytes_per_px + 3) & !3;

    let mut y = 0isize;
    while y < scr_h && y < 768 {
        let src_y = (y * img_h) / scr_h;
        let bmp_row = if top_down { src_y } else { img_h - 1 - src_y };
        let row_offset = data_off + (bmp_row as usize) * row_stride;
        let dst_row = fb.offset(y * scr_w);

        let mut x = 0isize;
        while x < scr_w && x < 1024 {
            let src_x = (x * img_w) / scr_w;
            let px_off = row_offset + (src_x as usize) * bytes_per_px;
            if px_off + 2 < bmp.len() {
                let b = read_volatile(p.add(px_off)) as u32;
                let g = read_volatile(p.add(px_off + 1)) as u32;
                let r = read_volatile(p.add(px_off + 2)) as u32;
                write_volatile(dst_row.offset(x), (r << 16) | (g << 8) | b);
            }
            x += 1;
        }
        y += 1;
    }
    true
}

unsafe fn apply_selected_wallpaper(b_ptr: *mut u32, scr_w: isize, scr_h: isize) {
    let bmp = match WALL_IDX {
        1 => LOGIN_WALL_2,
        2 => LOGIN_WALL_3,
        _ => LOGIN_WALL_1,
    };
    if draw_bmp_fullscreen(b_ptr, scr_w, scr_h, bmp) {
        return;
    }

    let mut y = 0isize;
    while y < scr_h && y < 768 {
        let col = match WALL_IDX {
            1 => {
                let r = 18 + ((y * 55) / scr_h) as u32;
                let g = 42 + ((y * 95) / scr_h) as u32;
                let b = 85 + ((y * 120) / scr_h) as u32;
                (r << 16) | (g << 8) | b
            }
            2 => {
                let r = 28 + ((y * 40) / scr_h) as u32;
                let g = 20 + ((y * 35) / scr_h) as u32;
                let b = 64 + ((y * 80) / scr_h) as u32;
                (r << 16) | (g << 8) | b
            }
            _ => {
                let r = 10 + ((y * 14) / scr_h) as u32;
                let g = 18 + ((y * 28) / scr_h) as u32;
                let b = 36 + ((y * 52) / scr_h) as u32;
                (r << 16) | (g << 8) | b
            }
        };
        let row = b_ptr.offset(y * scr_w);
        let mut x = 0isize;
        while x < scr_w && x < 1024 {
            write_volatile(row.offset(x), col);
            x += 1;
        }
        y += 1;
    }
}

unsafe fn draw_big_digit(fb: *mut u32, stride: isize, max_h: isize, x: isize, y: isize, d: u8, col: u32) {
    let segs: u8 = match d {
        0 => 0b0111111,
        1 => 0b0000110,
        2 => 0b1011011,
        3 => 0b1001111,
        4 => 0b1100110,
        5 => 0b1101101,
        6 => 0b1111101,
        7 => 0b0000111,
        8 => 0b1111111,
        9 => 0b1101111,
        _ => 0,
    };
    let w = 24;
    let h = 44;
    let t = 4;
    if (segs & 1) != 0 { fill_rect(fb, stride, max_h, x + 2, y, w - 4, t, col); }
    if (segs & 2) != 0 { fill_rect(fb, stride, max_h, x + w - t, y + 2, t, h / 2 - 2, col); }
    if (segs & 4) != 0 { fill_rect(fb, stride, max_h, x + w - t, y + h / 2 + 1, t, h / 2 - 3, col); }
    if (segs & 8) != 0 { fill_rect(fb, stride, max_h, x + 2, y + h - t, w - 4, t, col); }
    if (segs & 16) != 0 { fill_rect(fb, stride, max_h, x, y + h / 2 + 1, t, h / 2 - 3, col); }
    if (segs & 32) != 0 { fill_rect(fb, stride, max_h, x, y + 2, t, h / 2 - 2, col); }
    if (segs & 64) != 0 { fill_rect(fb, stride, max_h, x + 2, y + h / 2 - 2, w - 4, t, col); }
}

unsafe fn draw_big_clock(fb: *mut u32, stride: isize, max_h: isize, cx: isize, y: isize, hr: u8, min: u8) {
    let start_x = cx - 68;
    draw_big_digit(fb, stride, max_h, start_x, y, hr / 10, 0x00F8FAFC);
    draw_big_digit(fb, stride, max_h, start_x + 30, y, hr % 10, 0x00F8FAFC);

    fill_rect(fb, stride, max_h, start_x + 63, y + 12, 5, 5, 0x0038BDF8);
    fill_rect(fb, stride, max_h, start_x + 63, y + 28, 5, 5, 0x0038BDF8);

    draw_big_digit(fb, stride, max_h, start_x + 76, y, min / 10, 0x00F8FAFC);
draw_big_digit(fb, stride, max_h, start_x + 106, y, min % 10, 0x00F8FAFC);
}
fn scancode_to_ascii(sc: u8, shift: bool) -> u8 {
match sc {
0x02 => if shift { b'!' } else { b'1' },
0x03 => if shift { b'@' } else { b'2' },
0x04 => if shift { b'#' } else { b'3' },
0x05 => if shift { b'$' } else { b'4' },
0x06 => if shift { b'%' } else { b'5' },
0x07 => if shift { b'^' } else { b'6' },
0x08 => if shift { b'&' } else { b'7' },
0x09 => if shift { b'*' } else { b'8' },
0x0A => if shift { b'(' } else { b'9' },
0x0B => if shift { b')' } else { b'0' },
0x0C => if shift { b'_' } else { b'-' },
0x0D => if shift { b'+' } else { b'=' },
0x10 => if shift { b'Q' } else { b'q' },
0x11 => if shift { b'W' } else { b'w' },
0x12 => if shift { b'E' } else { b'e' },
0x13 => if shift { b'R' } else { b'r' },
0x14 => if shift { b'T' } else { b't' },
0x15 => if shift { b'Y' } else { b'y' },
0x16 => if shift { b'U' } else { b'u' },
0x17 => if shift { b'I' } else { b'i' },
0x18 => if shift { b'O' } else { b'o' },
0x19 => if shift { b'P' } else { b'p' },
0x1E => if shift { b'A' } else { b'a' },
0x1F => if shift { b'S' } else { b's' },
0x20 => if shift { b'D' } else { b'd' },
0x21 => if shift { b'F' } else { b'f' },
0x22 => if shift { b'G' } else { b'g' },
0x23 => if shift { b'H' } else { b'h' },
0x24 => if shift { b'J' } else { b'j' },
0x25 => if shift { b'K' } else { b'k' },
0x26 => if shift { b'L' } else { b'l' },
0x2C => if shift { b'Z' } else { b'z' },
0x2D => if shift { b'X' } else { b'x' },
0x2E => if shift { b'C' } else { b'c' },
0x2F => if shift { b'V' } else { b'v' },
0x30 => if shift { b'B' } else { b'b' },
0x31 => if shift { b'N' } else { b'n' },
0x32 => if shift { b'M' } else { b'm' },
0x39 => b' ',
_ => 0,
}
}
pub unsafe fn run_login_screen(fb: *mut u32, scr_w: isize, scr_h: isize, _wall_bmp: &[u8]) {
let b_ptr = LOGIN_BG_CACHE.as_mut_ptr();
apply_selected_wallpaper(b_ptr, scr_w, scr_h);
let mut input_pass = [0u8; 32];
let mut input_len: usize = 0;
let mut shift_held = false;
let mut wrong_pass = false;
let mut idle_frames: u32 = 0;
const IDLE_HIDE_THRESHOLD: u32 = 5400;
let mut mx = scr_w / 2;
let mut my = scr_h / 2 + 120;
let mut mouse_cycle = 0u8;
let mut m_bytes = [0u8; 3];
let mut prev_left = false;
loop {
while (inb(0x64) & 1) != 0 {
let status = inb(0x64);
let data = inb(0x60);
idle_frames = 0;
if (status & 0x20) != 0 {
let mb_ptr = m_bytes.as_mut_ptr();
if mouse_cycle == 0 && (data & 0x08) == 0 {
continue;
}
write_volatile(mb_ptr.add(mouse_cycle as usize), data);
mouse_cycle += 1;
if mouse_cycle == 3 {
mouse_cycle = 0;
let b0 = read_volatile(mb_ptr.add(0));
let dx = read_volatile(mb_ptr.add(1)) as i8 as isize;
let dy = read_volatile(mb_ptr.add(2)) as i8 as isize;
mx = (mx + dx).clamp(0, scr_w - 12);
my = (my - dy).clamp(0, scr_h - 12);
let left_now = (b0 & 1) != 0;
let just_clicked = left_now && !prev_left;
prev_left = left_now;
if just_clicked {
if mx >= scr_w - 134 && mx <= scr_w - 16 && my >= scr_h - 42 && my <= scr_h - 14 {
LOGIN_CFG_OPEN = !LOGIN_CFG_OPEN;
continue;
}
if LOGIN_CFG_OPEN {
let px = scr_w / 2 - 190;
let py = scr_h / 2 - 155;
let pw = 380isize;
let ph = 310isize;
if mx >= px + pw - 30 && mx <= px + pw - 8 && my >= py + 6 && my <= py + 26 {
LOGIN_CFG_OPEN = false;
continue;
}
if my >= py + 58 && my <= py + 88 {
if mx >= px + 16 && mx <= px + 126 {
WALL_IDX = 0;
apply_selected_wallpaper(b_ptr, scr_w, scr_h);
} else if mx >= px + 134 && mx <= px + 246 {
WALL_IDX = 1;
apply_selected_wallpaper(b_ptr, scr_w, scr_h);
} else if mx >= px + 254 && mx <= px + 364 {
WALL_IDX = 2;
apply_selected_wallpaper(b_ptr, scr_w, scr_h);
}
continue;
}
if my >= py + 118 && my <= py + 144 {
if mx >= px + 220 && mx <= px + 252 {
TZ_OFFSET = (TZ_OFFSET - 1).clamp(-12, 14);
} else if mx >= px + 324 && mx <= px + 356 {
TZ_OFFSET = (TZ_OFFSET + 1).clamp(-12, 14);
}
continue;
}
if my >= py + 174 && my <= py + 198 {
if mx >= px + 130 && mx <= px + 158 { CLOCK_OFF_X -= 20; }
else if mx >= px + 164 && mx <= px + 192 { CLOCK_OFF_X += 20; }
else if mx >= px + 298 && mx <= px + 326 { CLOCK_OFF_Y -= 15; }
else if mx >= px + 332 && mx <= px + 360 { CLOCK_OFF_Y += 15; }
continue;
}
if my >= py + 210 && my <= py + 234 {
if mx >= px + 130 && mx <= px + 158 { CARD_OFF_X -= 20; }
else if mx >= px + 164 && mx <= px + 192 { CARD_OFF_X += 20; }
else if mx >= px + 298 && mx <= px + 326 { CARD_OFF_Y -= 15; }
else if mx >= px + 332 && mx <= px + 360 { CARD_OFF_Y += 15; }
continue;
}
if mx >= px + 16 && mx <= px + 186 && my >= py + ph - 40 && my <= py + ph - 12 {
CLOCK_OFF_X = 0;
CLOCK_OFF_Y = 0;
CARD_OFF_X = 0;
CARD_OFF_Y = 0;
continue;
}
if mx >= px + pw - 116 && mx <= px + pw - 16 && my >= py + ph - 40 && my <= py + ph - 12 {
LOGIN_CFG_OPEN = false;
continue;
}
} else {
let card_x = (scr_w / 2 - 150 + CARD_OFF_X).clamp(10, scr_w - 310);
let card_y = (scr_h / 2 - 30 + CARD_OFF_Y).clamp(10, scr_h - 200);
if mx >= card_x + 40 && mx <= card_x + 260 && my >= card_y + 132 && my <= card_y + 164 {
if check_password(&input_pass, input_len) {
return;
} else {
wrong_pass = true;
input_len = 0;
}
}
}
}
}
} else {
if data == 0x2A || data == 0x36 {
shift_held = true;
} else if data == 0xAA || data == 0xB6 {
shift_held = false;
} else if (data & 0x80) == 0 {
if data == 0x01 {
LOGIN_CFG_OPEN = false;
} else if data == 0x1C && !LOGIN_CFG_OPEN {
if check_password(&input_pass, input_len) {
return;
} else {
wrong_pass = true;
input_len = 0;
}
} else if data == 0x0E && !LOGIN_CFG_OPEN {
if input_len > 0 {
input_len -= 1;
}
} else if !LOGIN_CFG_OPEN {
let ch = scancode_to_ascii(data, shift_held);
if ch != 0 && input_len < 24 {
write_volatile(input_pass.as_mut_ptr().add(input_len), ch);
input_len += 1;
wrong_pass = false;
}
}
}
}
}
let total_px = (scr_w * scr_h).min(1024 * 768) as usize;
let f_ptr = LOGIN_FRAME_BUF.as_mut_ptr();
let mut p_idx = 0usize;
while p_idx < total_px {
write_volatile(f_ptr.add(p_idx), read_volatile(b_ptr.add(p_idx)));
p_idx += 1;
}
if idle_frames < IDLE_HIDE_THRESHOLD {
let (hr, min, day, mon, yr) = get_rtc_datetime();
let clk_cx = (scr_w / 2 + CLOCK_OFF_X).clamp(80, scr_w - 80);
let clk_y = (54 + CLOCK_OFF_Y).clamp(16, scr_h - 100);
draw_big_clock(f_ptr, scr_w, scr_h, clk_cx, clk_y, hr, min);
let mut date_str = *b"00.00.2000  |  UTC+00";
let d_ptr = date_str.as_mut_ptr();
write_volatile(d_ptr.add(0), b'0' + (day / 10));
write_volatile(d_ptr.add(1), b'0' + (day % 10));
write_volatile(d_ptr.add(3), b'0' + (mon / 10));
write_volatile(d_ptr.add(4), b'0' + (mon % 10));
write_volatile(d_ptr.add(8), b'0' + (yr / 10));
write_volatile(d_ptr.add(9), b'0' + (yr % 10));
write_volatile(d_ptr.add(18), if TZ_OFFSET >= 0 { b'+' } else { b'-' });
let abs_tz = TZ_OFFSET.unsigned_abs();
write_volatile(d_ptr.add(19), b'0' + (abs_tz / 10));
write_volatile(d_ptr.add(20), b'0' + (abs_tz % 10));
if let Ok(dtxt) = core::str::from_utf8(&date_str) {
crate::kernels::kernel_64::draw_txt(f_ptr, clk_cx - 84, clk_y + 56, dtxt, 0x00CBD5E1);
}
let card_w = 300isize;
let card_h = 186isize;
let card_x = (scr_w / 2 - card_w / 2 + CARD_OFF_X).clamp(10, scr_w - card_w - 10);
let card_y = (scr_h / 2 - 30 + CARD_OFF_Y).clamp(10, scr_h - card_h - 10);
blend_glass_rect(f_ptr, scr_w, scr_h, card_x, card_y, card_w, card_h, 0x000F172A, 205);
fill_rect(f_ptr, scr_w, scr_h, card_x, card_y, card_w, 1, 0x0038BDF8);
let card_cx = card_x + card_w / 2;
fill_rect(f_ptr, scr_w, scr_h, card_cx - 22, card_y + 14, 44, 44, 0x000284C7);
crate::kernels::kernel_64::draw_txt(f_ptr, card_cx - 8, card_y + 31, "OS", 0x00FFFFFF);
let uname = get_saved_username();
let u_x = card_cx - ((uname.len() as isize) * 4);
crate::kernels::kernel_64::draw_txt(f_ptr, u_x, card_y + 68, uname, 0x00F8FAFC);
let box_x = card_x + 30;
let box_y = card_y + 90;
let box_w = card_w - 60;
fill_rect(f_ptr, scr_w, scr_h, box_x, box_y, box_w, 28, 0x00020617);
fill_rect(
f_ptr,
scr_w,
scr_h,
box_x,
box_y,
box_w,
1,
if wrong_pass { 0x00EF4444 } else { 0x0038BDF8 },
);
if input_len == 0 {
let hint = if wrong_pass { "Неверный пароль!" } else { "Введите пароль..." };
let hcol = if wrong_pass { 0x00EF4444 } else { 0x0064748B };
crate::kernels::kernel_64::draw_txt(f_ptr, box_x + 10, box_y + 9, hint, hcol);
} else {
let stars = [b'*'; 24];
let shown = input_len.min(24);
if let Ok(st_str) = core::str::from_utf8(&stars[..shown]) {
crate::kernels::kernel_64::draw_txt(f_ptr, box_x + 10, box_y + 10, st_str, 0x00F8FAFC);
}
}
let btn_x = card_x + 40;
let btn_y = card_y + 132;
let btn_w = card_w - 80;
let hov = !LOGIN_CFG_OPEN && mx >= btn_x && mx <= btn_x + btn_w && my >= btn_y && my <= btn_y + 32;
fill_rect(
f_ptr,
scr_w,
scr_h,
btn_x,
btn_y,
btn_w,
32,
if hov { 0x003B82F6 } else { 0x002563EB },
);
crate::kernels::kernel_64::draw_txt(f_ptr, card_cx - 20, btn_y + 11, "Войти", 0x00FFFFFF);
let cfg_x = scr_w - 134;
let cfg_y = scr_h - 42;
let cfg_hov = mx >= cfg_x && mx <= cfg_x + 118 && my >= cfg_y && my <= cfg_y + 28;
blend_glass_rect(
f_ptr,
scr_w,
scr_h,
cfg_x,
cfg_y,
118,
28,
if LOGIN_CFG_OPEN || cfg_hov { 0x000284C7 } else { 0x000F172A },
220,
);
fill_rect(f_ptr, scr_w, scr_h, cfg_x, cfg_y, 118, 1, 0x0038BDF8);
crate::kernels::kernel_64::draw_txt(f_ptr, cfg_x + 22, cfg_y + 9, "Настройки", 0x00F8FAFC);
if LOGIN_CFG_OPEN {
let px = scr_w / 2 - 190;
let py = scr_h / 2 - 155;
let pw = 380isize;
let ph = 310isize;
blend_glass_rect(f_ptr, scr_w, scr_h, px, py, pw, ph, 0x000F172A, 245);
fill_rect(f_ptr, scr_w, scr_h, px, py, pw, 1, 0x0038BDF8);
fill_rect(f_ptr, scr_w, scr_h, px + 1, py + 1, pw - 2, 28, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 14, py + 10, "Настройки экрана входа", 0x0038BDF8);
fill_rect(f_ptr, scr_w, scr_h, px + pw - 30, py + 6, 22, 20, 0x00991B1B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + pw - 23, py + 11, "X", 0x00FFFFFF);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 16, py + 38, "Обои экрана:", 0x00CBD5E1);
let w_names = ["Просторы", "Зимняя Сказка", "Горы"];
let w_xs = [px + 16, px + 134, px + 254];
let mut wi = 0usize;
while wi < 3 {
let bx = w_xs[wi];
let active = WALL_IDX == wi;
fill_rect(f_ptr, scr_w, scr_h, bx, py + 58, 110, 30, if active { 0x000284C7 } else { 0x001E293B });
if active {
fill_rect(f_ptr, scr_w, scr_h, bx, py + 58, 110, 2, 0x0038BDF8);
}
crate::kernels::kernel_64::draw_txt(f_ptr, bx + 8, py + 68, w_names[wi], 0x00F8FAFC);
wi += 1;
}
crate::kernels::kernel_64::draw_txt(f_ptr, px + 16, py + 125, "Часовой пояс (UTC):", 0x00CBD5E1);
fill_rect(f_ptr, scr_w, scr_h, px + 220, py + 118, 32, 26, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 232, py + 126, "-", 0x00FFFFFF);
let mut tz_str = *b"UTC+00";
let tz_ptr = tz_str.as_mut_ptr();
write_volatile(tz_ptr.add(3), if TZ_OFFSET >= 0 { b'+' } else { b'-' });
write_volatile(tz_ptr.add(4), b'0' + (abs_tz / 10));
write_volatile(tz_ptr.add(5), b'0' + (abs_tz % 10));
if let Ok(tzs) = core::str::from_utf8(&tz_str) {
crate::kernels::kernel_64::draw_txt(f_ptr, px + 264, py + 126, tzs, 0x0038BDF8);
}
fill_rect(f_ptr, scr_w, scr_h, px + 324, py + 118, 32, 26, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 336, py + 126, "+", 0x00FFFFFF);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 16, py + 181, "Часы X/Y:", 0x00CBD5E1);
fill_rect(f_ptr, scr_w, scr_h, px + 130, py + 174, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 140, py + 181, "<", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 164, py + 174, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 174, py + 181, ">", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 298, py + 174, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 308, py + 181, "^", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 332, py + 174, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 342, py + 181, "v", 0x00FFFFFF);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 16, py + 217, "Форма X/Y:", 0x00CBD5E1);
fill_rect(f_ptr, scr_w, scr_h, px + 130, py + 210, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 140, py + 217, "<", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 164, py + 210, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 174, py + 217, ">", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 298, py + 210, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 308, py + 217, "^", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 332, py + 210, 28, 24, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 342, py + 217, "v", 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, px + 16, py + ph - 40, 170, 28, 0x001E293B);
crate::kernels::kernel_64::draw_txt(f_ptr, px + 32, py + ph - 31, "Сбросить позицию", 0x00F8FAFC);
fill_rect(f_ptr, scr_w, scr_h, px + pw - 116, py + ph - 40, 100, 28, 0x002563EB);
crate::kernels::kernel_64::draw_txt(f_ptr, px + pw - 90, py + ph - 31, "Готово", 0x00FFFFFF);
}
fill_rect(f_ptr, scr_w, scr_h, mx, my, 10, 10, 0x00FFFFFF);
fill_rect(f_ptr, scr_w, scr_h, mx + 2, my + 2, 6, 6, 0x0038BDF8);
}
let qwords = total_px / 2;
let s64 = f_ptr as *const u64;
let d64 = fb as *mut u64;
let mut q = 0usize;
while q < qwords {
write_volatile(d64.add(q), read_volatile(s64.add(q)));
q += 1;
}
idle_frames = idle_frames.saturating_add(1);
}
}
unsafe fn check_password(input: &[u8; 32], len: usize) -> bool {
if SAVED_PASS_LEN == 0 {
return true;
}
if len != SAVED_PASS_LEN {
return false;
}
let a = input.as_ptr();
let b = SAVED_PASS.as_ptr();
let mut i = 0;
while i < len {
if read_volatile(a.add(i)) != read_volatile(b.add(i)) {
return false;
}
i += 1;
}
true
}
pub unsafe fn run_login_gate(fb: *mut u32, _bg_ptr: *const u32) {
let scr_w = crate::kernels::kernel_64::SCREEN_W;
let scr_h = crate::kernels::kernel_64::SCREEN_H;
if fb.is_null() {
return;
}
run_login_screen(fb, scr_w, scr_h, LOGIN_WALL_1);
}
