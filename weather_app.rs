#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

pub const WEATHER_W: isize = 360;
pub const WEATHER_H: isize = 270;

pub static mut WIN_WEATHER: bool = false;
pub static mut WEATHER_X: isize = 340;
pub static mut WEATHER_Y: isize = 130;
pub static mut CITY_IDX: usize = 0;

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

/// Рисует иконку погоды: 0 = Солнце, 1 = Облачно, 2 = Снег, 3 = Дождь
unsafe fn draw_weather_icon(fb: *mut u32, stride: isize, x: isize, y: isize, kind: u8) {
    match kind {
        0 => {
            // Солнце
            fill_rect(fb, stride, x + 10, y + 10, 28, 28, 0x00FACC15);
            fill_rect(fb, stride, x + 22, y + 2, 4, 6, 0x00FDE047);
            fill_rect(fb, stride, x + 22, y + 40, 4, 6, 0x00FDE047);
            fill_rect(fb, stride, x + 2, y + 22, 6, 4, 0x00FDE047);
            fill_rect(fb, stride, x + 40, y + 22, 6, 4, 0x00FDE047);
        }
        2 => {
            // Облако и снежинки
            fill_rect(fb, stride, x + 10, y + 12, 28, 14, 0x00CBD5E1);
            fill_rect(fb, stride, x + 6, y + 18, 36, 12, 0x00E2E8F0);
            fill_rect(fb, stride, x + 12, y + 34, 4, 4, 0x0038BDF8);
            fill_rect(fb, stride, x + 22, y + 38, 4, 4, 0x007DD3FC);
            fill_rect(fb, stride, x + 32, y + 34, 4, 4, 0x0038BDF8);
        }
        3 => {
            // Туча и дождь
            fill_rect(fb, stride, x + 10, y + 10, 28, 14, 0x0064748B);
            fill_rect(fb, stride, x + 6, y + 16, 36, 12, 0x0094A3B8);
            fill_rect(fb, stride, x + 12, y + 32, 2, 8, 0x0038BDF8);
            fill_rect(fb, stride, x + 22, y + 34, 2, 8, 0x0038BDF8);
            fill_rect(fb, stride, x + 32, y + 32, 2, 8, 0x0038BDF8);
        }
        _ => {
            // Переменная облачность
            fill_rect(fb, stride, x + 20, y + 6, 18, 18, 0x00FACC15);
            fill_rect(fb, stride, x + 8, y + 16, 26, 14, 0x00CBD5E1);
            fill_rect(fb, stride, x + 4, y + 22, 34, 12, 0x00F8FAFC);
        }
    }
}

pub unsafe fn draw_weather_window(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_WEATHER {
        return;
    }
    let wx = WEATHER_X;
    let wy = WEATHER_Y;
    let ww = WEATHER_W;
    let wh = WEATHER_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Погода midenOS — Метеосводка", 0x0038BDF8);

    // [X]
    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    // Переключатель городов
    let cities = ["Бузулук", "Москва", "Екатеринбург", "Сочи"];
    let c_ptr = cities.as_ptr();
    let mut ci = 0isize;
    while ci < 4 {
        let bx = wx + 12 + ci * 85;
        let active = CITY_IDX == (ci as usize);
        fill_rect(fb, stride, bx, wy + 36, 80, 24, if active { 0x000284C7 } else { 0x001E293B });
        let cname = read_volatile(c_ptr.add(ci as usize));
        crate::kernels::kernel_64::draw_txt(fb, bx + 6, wy + 43, cname, 0x00F8FAFC);
        ci += 1;
    }

    // Определяем сезон по месяцу BIOS RTC
    let mon = read_cmos(0x08);
    let is_winter = mon == 12 || mon == 1 || mon == 2;
    let is_autumn_spring = mon == 3 || mon == 4 || mon == 10 || mon == 11;

    let (temp_str, cond_str, wind_str, icon_kind, f1, f2, f3) = match CITY_IDX {
        1 => {
            if is_winter { ("-6 C", "Снег, пасмурно", "Ветер: 4 м/с СЗ  |  Влажность: 84%", 2, "Завтра: -8 C", "Сб: -5 C", "Вс: -4 C") }
            else if is_autumn_spring { ("+4 C", "Облачно, морось", "Ветер: 5 м/с З   |  Влажность: 79%", 3, "Завтра: +5 C", "Сб: +3 C", "Вс: +6 C") }
            else { ("+23 C", "Малооблачно", "Ветер: 3 м/с Ю   |  Влажность: 52%", 1, "Завтра: +25 C", "Сб: +24 C", "Вс: +21 C") }
        }
        2 => {
            if is_winter { ("-14 C", "Ясно, морозно", "Ветер: 3 м/с В   |  Влажность: 76%", 2, "Завтра: -16 C", "Сб: -12 C", "Вс: -11 C") }
            else if is_autumn_spring { ("+1 C", "Снег с дождём", "Ветер: 6 м/с С   |  Влажность: 82%", 2, "Завтра: -1 C", "Сб: +2 C", "Вс: +4 C") }
            else { ("+21 C", "Солнечно", "Ветер: 3 м/с СЗ  |  Влажность: 48%", 0, "Завтра: +22 C", "Сб: +19 C", "Вс: +20 C") }
        }
        3 => {
            if is_winter { ("+9 C", "Облачно, без осадков", "Ветер: 4 м/с ЮВ  |  Влажность: 71%", 1, "Завтра: +11 C", "Сб: +10 C", "Вс: +12 C") }
            else if is_autumn_spring { ("+15 C", "Солнечно, бриз", "Ветер: 3 м/с Ю   |  Влажность: 64%", 0, "Завтра: +16 C", "Сб: +17 C", "Вс: +15 C") }
            else { ("+28 C", "Ясно, жарко", "Ветер: 2 м/с ЮЗ  |  Влажность: 58%", 0, "Завтра: +29 C", "Сб: +28 C", "Вс: +27 C") }
        }
        _ => {
            // Бузулук (по умолчанию)
            if is_winter { ("-11 C", "Снег, лёгкий ветер", "Ветер: 4 м/с СВ  |  Влажность: 81%", 2, "Завтра: -13 C", "Сб: -9 C", "Вс: -8 C") }
            else if is_autumn_spring { ("+2 C", "Переменная облачность", "Ветер: 5 м/с З   |  Влажность: 74%", 1, "Завтра: +3 C", "Сб: +1 C", "Вс: +4 C") }
            else { ("+26 C", "Ясно, солнечно", "Ветер: 3 м/с Ю   |  Влажность: 45%", 0, "Завтра: +28 C", "Сб: +27 C", "Вс: +25 C") }
        }
    };

    // Карточка текущей погоды
    fill_rect(fb, stride, wx + 14, wy + 70, ww - 28, 92, 0x001E293B);
    draw_weather_icon(fb, stride, wx + 26, wy + 90, icon_kind);

    crate::kernels::kernel_64::draw_txt(fb, wx + 92, wy + 84, temp_str, 0x00F8FAFC);
    crate::kernels::kernel_64::draw_txt(fb, wx + 93, wy + 84, temp_str, 0x00F8FAFC);
    crate::kernels::kernel_64::draw_txt(fb, wx + 92, wy + 106, cond_str, 0x0038BDF8);
    crate::kernels::kernel_64::draw_txt(fb, wx + 26, wy + 142, wind_str, 0x0094A3B8);

    // Прогноз на 3 дня снизу
    let days = [f1, f2, f3];
    let d_ptr = days.as_ptr();
    let mut di = 0isize;
    while di < 3 {
        let fx = wx + 14 + di * 112;
        fill_rect(fb, stride, fx, wy + 174, 106, 54, 0x001E293B);
        let dtxt = read_volatile(d_ptr.add(di as usize));
        crate::kernels::kernel_64::draw_txt(fb, fx + 10, wy + 195, dtxt, 0x00E2E8F0);
        di += 1;
    }

    crate::kernels::kernel_64::draw_txt(fb, wx + 14, wy + wh - 22, "Источник: Датчики RTC + Offline Meteo", 0x0064748B);
}

pub unsafe fn handle_weather_click(
    mx: isize,
    my: isize,
    drag_win: &mut u8,
    drag_off_x: &mut isize,
    drag_off_y: &mut isize,
    focused_win: &mut u8,
) -> bool {
    if !WIN_WEATHER {
        return false;
    }
    let wx = WEATHER_X;
    let wy = WEATHER_Y;
    let ww = WEATHER_W;
    let wh = WEATHER_H;

    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        return false;
    }

    *focused_win = 13;

    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_WEATHER = false;
        return true;
    }

    if my >= wy && my <= wy + 28 {
        *drag_win = 13;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }

    // Выбор города
    if my >= wy + 36 && my <= wy + 60 {
        let mut ci = 0isize;
        while ci < 4 {
            let bx = wx + 12 + ci * 85;
            if mx >= bx && mx <= bx + 80 {
                CITY_IDX = ci as usize;
                return true;
            }
            ci += 1;
        }
    }

    true
}
