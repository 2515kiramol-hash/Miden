#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

pub const CAL_W: isize = 350;
pub const CAL_H: isize = 310;

pub static mut WIN_CALENDAR: bool = false;
pub static mut CAL_X: isize = 180;
pub static mut CAL_Y: isize = 90;

static mut SEL_DAY: u8 = 19;
static mut VIEW_MONTH: u8 = 0; // 0 = текущий месяц из BIOS RTC
static mut VIEW_YEAR: u16 = 0;
static mut DAY_NOTES: [u8; 32] = [0; 32]; // 0 = нет, 1 = Важно, 2 = Встреча, 3 = Релиз ОС, 4 = Дедлайн

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

fn days_in_month(m: u8, y: u16) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0) { 29 } else { 28 },
        _ => 30,
    }
}

// День недели для 1-го числа месяца (0 = Пн .. 6 = Вс) по алгоритму Сакамото
fn first_weekday_mon0(m: u8, y: u16) -> usize {
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut yr = y as i32;
    if m < 3 { yr -= 1; }
    let sun0 = (yr + yr / 4 - yr / 100 + yr / 400 + t[(m.clamp(1, 12) - 1) as usize] + 1) % 7;
    ((sun0 + 6) % 7) as usize
}

pub unsafe fn draw_calendar_window(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_CALENDAR { return; }

    let rtc_day = read_cmos(0x07).clamp(1, 31);
    let rtc_mon = read_cmos(0x08).clamp(1, 12);
    let rtc_yr = 2000u16 + (read_cmos(0x09) as u16);

    if VIEW_MONTH == 0 {
        VIEW_MONTH = rtc_mon;
        VIEW_YEAR = rtc_yr;
        SEL_DAY = rtc_day;
    }

    let wx = CAL_X;
    let wy = CAL_Y;
    let ww = CAL_W;
    let wh = CAL_H;

    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    fill_rect(fb, stride, wx, wy, ww, 1, if focused { 0x0038BDF8 } else { 0x00334155 });

    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 28, if focused { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 12, wy + 9, "Календарь midenOS", 0x0038BDF8);

    // [X]
    let cx = wx + ww - 28;
    let cy = wy + 4;
    let chov = mx >= cx && mx <= cx + 22 && my >= cy && my <= cy + 20;
    fill_rect(fb, stride, cx, cy, 22, 20, if chov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, cx + 7, cy + 6, "X", 0x00FFFFFF);

    // Переключатель месяца
    fill_rect(fb, stride, wx + 14, wy + 34, 28, 22, 0x001E293B);
    crate::kernels::kernel_64::draw_txt(fb, wx + 24, wy + 40, "<", 0x00FFFFFF);

    let m_names = [
        "Январь", "Февраль", "Март", "Апрель", "Май", "Июнь",
        "Июль", "Август", "Сентябрь", "Октябрь", "Ноябрь", "Декабрь",
    ];
    let m_str = m_names[(VIEW_MONTH.clamp(1, 12) - 1) as usize];
    crate::kernels::kernel_64::draw_txt(fb, wx + 115, wy + 40, m_str, 0x00F8FAFC);

    fill_rect(fb, stride, wx + ww - 42, wy + 34, 28, 22, 0x001E293B);
    crate::kernels::kernel_64::draw_txt(fb, wx + ww - 32, wy + 40, ">", 0x00FFFFFF);

    // Заголовки дней недели
    let wdays = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
    let mut c = 0isize;
    while c < 7 {
        let col = if c >= 5 { 0x00F87171 } else { 0x0094A3B8 };
        crate::kernels::kernel_64::draw_txt(fb, wx + 22 + c * 46, wy + 64, wdays[c as usize], col);
        c += 1;
    }

    // Сетка дней
    let start_col = first_weekday_mon0(VIEW_MONTH, VIEW_YEAR);
    let max_days = days_in_month(VIEW_MONTH, VIEW_YEAR);
    let notes_ptr = DAY_NOTES.as_ptr();

    let mut d = 1u8;
    while d <= max_days {
        let idx = start_col + ((d - 1) as usize);
        let row = (idx / 7) as isize;
        let col = (idx % 7) as isize;
        let bx = wx + 14 + col * 46;
        let by = wy + 82 + row * 28;

        let is_today = d == rtc_day && VIEW_MONTH == rtc_mon && VIEW_YEAR == rtc_yr;
        let is_sel = d == SEL_DAY;
        let note_tag = read_volatile(notes_ptr.add(d as usize));

        let bg = if is_today {
            0x000284C7
        } else if is_sel {
            0x00334155
        } else {
            0x001E293B
        };
        fill_rect(fb, stride, bx, by, 42, 24, bg);
        if is_sel {
            fill_rect(fb, stride, bx, by, 42, 1, 0x0038BDF8);
        }

        // Маркер заметки на дне
        if note_tag > 0 {
            let ncol = match note_tag {
                1 => 0x00EF4444,
                2 => 0x00FACC15,
                3 => 0x004ADE80,
                _ => 0x00C084FC,
            };
            fill_rect(fb, stride, bx + 34, by + 4, 4, 4, ncol);
        }

        let mut d_txt = [b' ', b'0'];
        if d >= 10 {
            d_txt[0] = b'0' + (d / 10);
            d_txt[1] = b'0' + (d % 10);
        } else {
            d_txt[0] = b'0' + d;
            d_txt[1] = b' ';
        }
        if let Ok(s) = core::str::from_utf8(&d_txt) {
            crate::kernels::kernel_64::draw_txt(fb, bx + 12, by + 7, s, 0x00F8FAFC);
        }
        d += 1;
    }

    // Нижняя панель заметки для выбранного дня
    let ny = wy + wh - 46;
    fill_rect(fb, stride, wx + 14, ny, ww - 134, 32, 0x00020617);
    let cur_tag = read_volatile(notes_ptr.add(SEL_DAY.clamp(1, 31) as usize));
    let note_label = match cur_tag {
        1 => "Заметка: Важное событие",
        2 => "Заметка: Рабочая встреча",
        3 => "Заметка: Релиз midenOS",
        4 => "Заметка: Напоминание",
        _ => "Нет заметки на дату",
    };
    crate::kernels::kernel_64::draw_txt(fb, wx + 22, ny + 11, note_label, 0x00CBD5E1);

    let bhov = mx >= wx + ww - 114 && mx <= wx + ww - 14 && my >= ny && my <= ny + 32;
    fill_rect(fb, stride, wx + ww - 114, ny, 100, 32, if bhov { 0x002563EB } else { 0x001D4ED8 });
    crate::kernels::kernel_64::draw_txt(fb, wx + ww - 102, ny + 11, "Метка +/-", 0x00FFFFFF);
}

pub unsafe fn handle_calendar_click(
    mx: isize, my: isize,
    drag_win: &mut u8, drag_off_x: &mut isize, drag_off_y: &mut isize, focused_win: &mut u8,
) -> bool {
    if !WIN_CALENDAR { return false; }
    let wx = CAL_X;
    let wy = CAL_Y;
    let ww = CAL_W;
    let wh = CAL_H;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh { return false; }

    *focused_win = 14;
    if mx >= wx + ww - 28 && mx <= wx + ww - 6 && my >= wy + 4 && my <= wy + 24 {
        WIN_CALENDAR = false;
        return true;
    }
    if my >= wy && my <= wy + 28 {
        *drag_win = 14;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }
    if my >= wy + 34 && my <= wy + 56 {
        if mx >= wx + 14 && mx <= wx + 42 {
            if VIEW_MONTH == 1 { VIEW_MONTH = 12; VIEW_YEAR -= 1; } else { VIEW_MONTH -= 1; }
            return true;
        }
        if mx >= wx + ww - 42 && mx <= wx + ww - 14 {
            if VIEW_MONTH == 12 { VIEW_MONTH = 1; VIEW_YEAR += 1; } else { VIEW_MONTH += 1; }
            return true;
        }
    }

    // Клик по ячейке дня
    let start_col = first_weekday_mon0(VIEW_MONTH, VIEW_YEAR);
    let max_days = days_in_month(VIEW_MONTH, VIEW_YEAR);
    let mut d = 1u8;
    while d <= max_days {
        let idx = start_col + ((d - 1) as usize);
        let bx = wx + 14 + ((idx % 7) as isize) * 46;
        let by = wy + 82 + ((idx / 7) as isize) * 28;
        if mx >= bx && mx <= bx + 42 && my >= by && my <= by + 24 {
            SEL_DAY = d;
            return true;
        }
        d += 1;
    }

    // Кнопка смены заметки на выбранный день
    let ny = wy + wh - 46;
    if mx >= wx + ww - 114 && mx <= wx + ww - 14 && my >= ny && my <= ny + 32 {
        let p = DAY_NOTES.as_mut_ptr().add(SEL_DAY.clamp(1, 31) as usize);
        write_volatile(p, (read_volatile(p) + 1) % 5);
        return true;
    }

    true
}
