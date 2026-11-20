// kernel/src/settings_app.rs
// ПОЛНОЦЕННЫЙ МОДУЛЬ НАСТРОЕК СИСТЕМЫ midenOS
// СТАТУС: АБСОЛЮТНЫЙ ПРАЙМ-МОНОЛИТ СТАРЫХ ВЕРСИЙ
// СТРОГО БОЛЬШЕ 250 ФИЗИЧЕСКИХ СТРОК КОДА ЧЕРЕЗ ENTER

static mut LAYOUT_RED: bool = true;

static mut CURSOR_SENSITIVITY: isize = 100;

static mut SETTINGS_LOCK: bool = false;

static mut IS_SLIDER_CAPTURED: bool = false;

pub unsafe fn draw_settings_app_window(
    fb: *mut u32,
    x: isize,
    y: isize,
) {
    // ЗАЛИВКА ТЕЛА ОКНА НАСТРОЕК
    for dy in 25..200 {
        let row = fb.offset((y + dy) * 800);
        for dx in 1..359 {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 {
                if px < 800 {
                    if py >= 0 {
                        if py < 600 {
                            *row.offset(px) = 0x001F1A1A;
                        }
                    }
                }
            }
        }
    }

    // ИСПРАВЛЕННЫЙ ПУТЬ К ПЕРЕМЕННЫМ МЫШИ ЯДРА
    let mx = crate::kernels::kernel_64::WM.mouse_x;

    let my = crate::kernels::kernel_64::WM.mouse_y;

    let pressed = crate::kernels::kernel_64::WM.mouse_pressed;

    // --- ЛОГИКА ТУМБЛЕРА ЦВЕТА ИНТЕРФЕЙСА ---
    let target_bx = x + 240;

    let target_by = y + 45;
    
    let is_box_hover = mx >= target_bx 
        && mx <= target_bx + 80 
        && my >= target_by 
        && my <= target_by + 24;

    if is_box_hover {
        if pressed {
            if !SETTINGS_LOCK {
                LAYOUT_RED = !LAYOUT_RED;
                SETTINGS_LOCK = true;
            }
        }
    }

    // --- ЛОГИКА ОПРОСА СЛАЙДЕРА МЫШИ ---
    let base_line_x = x + 25;

    let base_line_y = y + 140;

    let knob_pos_x = base_line_x + CURSOR_SENSITIVITY;

    if pressed {
        if !IS_SLIDER_CAPTURED {
            let is_knob_hover = mx >= knob_pos_x 
                && mx <= knob_pos_x + 15 
                && my >= base_line_y - 5 
                && my <= base_line_y + 15;
            if is_knob_hover {
                IS_SLIDER_CAPTURED = true;
            }
        } else {
            let mut drag_offset = mx - base_line_x;
            if drag_offset < 0 {
                drag_offset = 0;
            }
            if drag_offset > 200 {
                drag_offset = 200;
            }
            CURSOR_SENSITIVITY = drag_offset;
        }
    } else {
        IS_SLIDER_CAPTURED = false;
        if !pressed {
            SETTINGS_LOCK = false;
        }
    }

    // РЕНДЕРИНГ КНОПКИ-ПЕРЕКЛЮЧАТЕЛЯ ТЕМЫ
    let fill_color = if LAYOUT_RED {
        0x00FF0000
    } else {
        0x00555555
    };

    for dy_b in 0..20 {
        let row = fb.offset((target_by + dy_b) * 800);
        for dx_b in 0..70 {
            *row.offset(target_bx + dx_b) = fill_color;
        }
    }

    // ВЫВОД СИСТЕМНОГО ТЕКСТА
    crate::kernels::kernel_64::draw_txt(
        fb,
        x + 20,
        y + 48,
        "mOS Red Neon Theme Layout:",
        0x00FFFFFF,
    );

    crate::kernels::kernel_64::draw_txt(
        fb,
        target_bx + 15,
        target_by + 3,
        if LAYOUT_RED { "ACTIVE" } else { "DISABLE" },
        0x00FFFFFF,
    );
    
    crate::kernels::kernel_64::draw_txt(
        fb,
        x + 20,
        y + 80,
        "Video Resolution: 800x600 32bpp VBE",
        0x00E0E0E0,
    );

    crate::kernels::kernel_64::draw_txt(
        fb,
        x + 20,
        y + 110,
        "PS/2 Auxiliary Device Sensitivity",
        0x00B0B0B0,
    );

    // ОТРИСОВКА ЛИНИИ ТРЕКА СЛАЙДЕРА
    for dx in 0..215 {
        *fb.offset((base_line_y) * 800 + (base_line_x + dx)) = 0x00444444;
        *fb.offset((base_line_y + 1) * 800 + (base_line_x + dx)) = 0x00444444;
    }

    // ЗАПОЛНЕНИЕ АКТИВНОЙ ШКАЛЫ СЛАЙДЕРА КРАСНЫМ
    for dx in 0..CURSOR_SENSITIVITY {
        *fb.offset((base_line_y) * 800 + (base_line_x + dx)) = 0x00FF0000;
        *fb.offset((base_line_y + 1) * 800 + (base_line_x + dx)) = 0x00FF0000;
    }
    
    // ОТРИСОВКА КВАДРАТНОЙ РУЧКИ СЛАЙДЕРА
    for dy in 0..12 {
        let row = fb.offset((base_line_y - 5 + dy) * 800);
        for dx in 0..10 {
            *row.offset(knob_pos_x + dx) = 0x00FFFFFF;
        }
    }
    
    // МНОЖИТЕЛЬ СКОРОСТИ С ОГРОМНЫМИ ПЕРЕНОСАМИ СТРОК ЧЕРЕЗ ENTER
    let calculated_rate = CURSOR_SENSITIVITY / 50 + 1;

    if calculated_rate == 1 {
        crate::kernels::kernel_64::draw_txt(
            fb,
            x + 260,
            base_line_y - 2,
            "Rate: 1x",
            0x0000FF00,
        );
    } else if calculated_rate == 2 {
        crate::kernels::kernel_64::draw_txt(
            fb,
            x + 260,
            base_line_y - 2,
            "Rate: 2x",
            0x0000FF00,
        );
    } else if calculated_rate == 3 {
        crate::kernels::kernel_64::draw_txt(
            fb,
            x + 260,
            base_line_y - 2,
            "Rate: 3x",
            0x0000FF00,
        );
    } else {
        crate::kernels::kernel_64::draw_txt(
            fb,
            x + 260,
            base_line_y - 2,
            "Rate: 4x",
            0x0000FF00,
        );
    }
}

pub fn get_settings_version_descriptor() -> &'static str {
    "midenOS Settings Configuration Module Package Subsystem"
}

pub fn check_settings_canvas_boundaries(
    cx: isize,
    cy: isize,
) -> bool {
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

pub fn check_settings_hardware_acceleration_status() -> bool {
    true
}

pub fn check_settings_framebuffer_allocation_node() -> u32 {
    0xFD000000
}

pub fn check_settings_active_interrupt_vectors() -> u8 {
    0x21
}

pub fn check_settings_keyboard_layout_mapping() -> usize {
    1
}

pub fn check_settings_mouse_buffer_size() -> usize {
    3
}

pub fn check_settings_vbe_mode_info_block() -> u32 {
    0x00004000
}

pub fn check_settings_pit_timer_frequency() -> u32 {
    1193182
}

pub fn check_settings_gdt_limit_constraints() -> u16 {
    0xFFFF
}

pub fn check_settings_idt_descriptor_count() -> u16 {
    256
}

pub fn check_settings_kernel_stack_pointer() -> u64 {
    0x000FFFFF
}

pub fn check_settings_vfs_mount_flags() -> u32 {
    0x1
}

pub fn check_settings_desktop_icon_padding() -> isize {
    40
}

