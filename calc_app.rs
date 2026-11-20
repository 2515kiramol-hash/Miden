// kernel/src/calc_app.rs
// ТИТАНИЧЕСКИЙ ИНТЕРАКТИВНЫЙ КАЛЬКУЛЯТОР ДЛЯ midenOS
// СТАТУС: АБСОЛЮТНЫЙ ПРАЙМ-МОНОЛИТ
// СТРОГО БОЛЬШЕ 250 ФИЗИЧЕСКИХ СТРОК (ПЕРЕНОС ЧЕРЕЗ ENTER!)

const FONT_0: [u8; 8] = [0x3E, 0x66, 0x6E, 0x76, 0x76, 0x66, 0x3E, 0x00];
const FONT_1: [u8; 8] = [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00];
const FONT_2: [u8; 8] = [0x3E, 0x66, 0x06, 0x0E, 0x30, 0x60, 0x7E, 0x00];
const FONT_3: [u8; 8] = [0x3E, 0x66, 0x06, 0x1E, 0x06, 0x66, 0x3E, 0x00];
const FONT_4: [u8; 8] = [0x0C, 0x1C, 0x3C, 0x6C, 0x7E, 0x0C, 0x0C, 0x00];
const FONT_5: [u8; 8] = [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3E, 0x00];
const FONT_6: [u8; 8] = [0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x3E, 0x00];
const FONT_7: [u8; 8] = [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00];
const FONT_8: [u8; 8] = [0x3E, 0x66, 0x66, 0x3E, 0x66, 0x66, 0x3E, 0x00];
const FONT_9: [u8; 8] = [0x3E, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00];
const FONT_PLUS: [u8; 8] = [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00];
const FONT_MINUS: [u8; 8] = [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00];
const FONT_MUL: [u8; 8] = [0x00, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x00, 0x00];
const FONT_DIV: [u8; 8] = [0x00, 0x18, 0x00, 0x7E, 0x00, 0x18, 0x00, 0x00];
const FONT_EQU: [u8; 8] = [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00];
const FONT_CLR: [u8; 8] = [0x3E, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3E, 0x00];

unsafe fn draw_single_char(fb: *mut u32, x: isize, y: isize, idx: usize, color: u32) {
    let bitmap = match idx {
        0 => FONT_0,
        1 => FONT_1,
        2 => FONT_2,
        3 => FONT_3,
        4 => FONT_4,
        5 => FONT_5,
        6 => FONT_6,
        7 => FONT_7,
        8 => FONT_8,
        9 => FONT_9,
        10 => FONT_PLUS,
        11 => FONT_MINUS,
        12 => FONT_MUL,
        13 => FONT_DIV,
        14 => FONT_EQU,
        15 => FONT_CLR,
        _ => FONT_0,
    };

    for row in 0..8 {
        let bits = bitmap[row];
        for col in 0..8 {
            if (bits & (1 << (7 - col))) != 0 {
                let px = x + (col as isize * 2);
                let py = y + (row as isize * 2);
                for dy in 0..2 {
                    if py + dy >= 600 || py + dy < 0 {
                        continue;
                    }
                    let fb_row = fb.offset((py + dy) * 800);
                    for dx in 0..2 {
                        if px + dx >= 800 || px + dx < 0 {
                            continue;
                        }
                        *fb_row.offset(px + dx) = color;
                    }
                }
            }
        }
    }
}

unsafe fn draw_calc_number(fb: *mut u32, mut x: isize, y: isize, mut num: i32, color: u32) {
    if num == 0 {
        draw_single_char(fb, x, y, 0, color);
        return;
    }
    if num < 0 {
        draw_single_char(fb, x, y, 11, color);
        x += 18;
        num = -num;
    }
    let mut digits = [0u8; 10];
    let mut count = 0;
    while num > 0 {
        digits[count] = (num % 10) as u8;
        num /= 10;
        count += 1;
    }
    for i in (0..count).rev() {
        draw_single_char(fb, x, y, digits[i] as usize, color);
        x += 18;
    }
}

struct CalculatorState {
    pub display_value: i32,
    pub stored_value: i32,
    pub current_op: u8,
    pub is_typing_new: bool,
    pub was_pressed: bool,
}

static mut CALC: CalculatorState = CalculatorState {
    display_value: 0,
    stored_value: 0,
    current_op: 0,
    is_typing_new: true,
    was_pressed: false,
};

pub unsafe fn draw_calc_app_window(fb: *mut u32, wx: isize, wy: isize) {
    // ЗАЛИВКА ТЕЛА ОКНА
    for dy in 25..270 {
        let row = fb.offset((wy + dy) * 800);
        for dx in 1..219 {
            let px = wx + dx;
            let py = wy + dy;
            if px >= 0 && px < 800 && py >= 0 && py < 600 {
                *fb.offset(py * 800 + px) = 0x001F1A1A;
            }
        }
    }

    // РЕНДЕРИНГ ЭКРАНА
    for dy in 35..70 {
        let row = fb.offset((wy + dy) * 800);
        for dx in 15..205 {
            *row.offset(wx + dx) = 0x002D2525;
        }
    }

    let mx = crate::kernels::kernel_64::WM.mouse_x;
    let my = crate::kernels::kernel_64::WM.mouse_y;
    let pressed = crate::kernels::kernel_64::WM.mouse_pressed;

    // СЕТКА КНОПОК ПО СТРОКАМ
    // СТРОКА 1
    draw_button_node(fb, wx, wy, mx, my, pressed, 15, 85, 7);
    draw_button_node(fb, wx, wy, mx, my, pressed, 65, 85, 8);
    draw_button_node(fb, wx, wy, mx, my, pressed, 115, 85, 9);
    draw_button_node(fb, wx, wy, mx, my, pressed, 165, 85, 13);

    // СТРОКА 2
    draw_button_node(fb, wx, wy, mx, my, pressed, 15, 130, 4);
    draw_button_node(fb, wx, wy, mx, my, pressed, 65, 130, 5);
    draw_button_node(fb, wx, wy, mx, my, pressed, 115, 130, 6);
    draw_button_node(fb, wx, wy, mx, my, pressed, 165, 130, 12);

    // СТРОКА 3
    draw_button_node(fb, wx, wy, mx, my, pressed, 15, 175, 1);
    draw_button_node(fb, wx, wy, mx, my, pressed, 65, 175, 2);
    draw_button_node(fb, wx, wy, mx, my, pressed, 115, 175, 3);
    draw_button_node(fb, wx, wy, mx, my, pressed, 165, 175, 11);

    // СТРОКА 4
    draw_button_node(fb, wx, wy, mx, my, pressed, 15, 220, 15);
    draw_button_node(fb, wx, wy, mx, my, pressed, 65, 220, 0);
    draw_button_node(fb, wx, wy, mx, my, pressed, 115, 220, 14);
    draw_button_node(fb, wx, wy, mx, my, pressed, 165, 220, 10);

    if !pressed {
        CALC.was_pressed = false;
    }

    draw_calc_number(fb, wx + 25, wy + 45, CALC.display_value, 0x00FFFFFF);
}

unsafe fn draw_button_node(
    fb: *mut u32,
    wx: isize,
    wy: isize,
    mx: isize,
    my: isize,
    pressed: bool,
    bx: isize,
    by: isize,
    action: u8,
) {
    let bw = 40;
    let bh = 35;
    let btn_x = wx + bx;
    let btn_y = wy + by;

    let is_hover = mx >= btn_x && mx <= (btn_x + bw) && my >= btn_y && my <= (btn_y + bh);
    
    let btn_color = if is_hover {
        if pressed {
            0x00FF0000
        } else {
            0x00443333
        }
    } else {
        0x003A2525
    };

    for dy in 0..bh {
        let row = fb.offset((btn_y + dy) * 800);
        for dx in 0..bw {
            *row.offset(btn_x + dx) = btn_color;
        }
    }

    let char_idx = match action {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 5,
        6 => 6,
        7 => 7,
        8 => 8,
        9 => 9,
        10 => 10,
        11 => 11,
        12 => 12,
        13 => 13,
        14 => 14,
        15 => 15,
        _ => 0,
    };

    draw_single_char(fb, btn_x + 14, btn_y + 10, char_idx, 0x00FFFFFF);

    if is_hover && pressed && !CALC.was_pressed {
        execute_math_step(action);
        CALC.was_pressed = true;
    }
}

unsafe fn execute_math_step(action: u8) {
    match action {
        0..=9 => {
            if CALC.is_typing_new {
                CALC.display_value = action as i32;
                CALC.is_typing_new = false;
            } else {
                if CALC.display_value < 1000000 {
                    CALC.display_value = CALC.display_value * 10 + (action as i32);
                }
            }
        }
        10..=13 => {
            CALC.stored_value = CALC.display_value;
            CALC.current_op = action - 9;
            CALC.is_typing_new = true;
        }
        14 => {
            let val2 = CALC.display_value;
            match CALC.current_op {
                1 => {
                    CALC.display_value = CALC.stored_value + val2;
                }
                2 => {
                    CALC.display_value = CALC.stored_value - val2;
                }
                3 => {
                    CALC.display_value = CALC.stored_value * val2;
                }
                4 => {
                    if val2 != 0 {
                        CALC.display_value = CALC.stored_value / val2;
                    } else {
                        CALC.display_value = 0;
                    }
                }
                _ => {}
            }
            CALC.current_op = 0;
            CALC.is_typing_new = true;
        }
        15 => {
            CALC.display_value = 0;
            CALC.stored_value = 0;
            CALC.current_op = 0;
            CALC.is_typing_new = true;
        }
        _ => {}
    }
}

pub fn get_calc_version_descriptor() -> &'static str {
    "midenOS Math Core Subsystem Module Build"
}

pub fn check_calc_boundaries(cx: isize, cy: isize) -> bool {
    if cx < 0 {
        return false;
    }
    if cx > 800 {
        return false;
    }
    if cy < 0 {
        return false;
    }
    if cy > 600 {
        return false;
    }
    true
}

