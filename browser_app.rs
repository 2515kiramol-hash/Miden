#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

pub const BROWSER_W: isize = 640;
pub const BROWSER_H: isize = 440;

pub static mut WIN_BROWSER: bool = false;
pub static mut BROWSER_X: isize = 160;
pub static mut BROWSER_Y: isize = 60;

pub static mut URL_BUF: [u8; 64] = [0u8; 64];
pub static mut URL_LEN: usize = 0;
pub static mut URL_FOCUSED: bool = false;

pub static mut PAGE_BUF: [u8; 4096] = [0u8; 4096];
pub static mut PAGE_LEN: usize = 0;
pub static mut SCROLL_LINE: usize = 0;
pub static mut DARK_PAGE: bool = false;
pub static mut ACTIVE_TAB: usize = 0;
pub static mut STATUS_MSG: &'static str = "Готов • Выберите закладку или введите адрес";

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

unsafe fn draw_box_border(fb: *mut u32, stride: isize, x: isize, y: isize, w: isize, h: isize, color: u32) {
    fill_rect(fb, stride, x, y, w, 1, color);
    fill_rect(fb, stride, x, y + h - 1, w, 1, color);
    fill_rect(fb, stride, x, y, 1, h, color);
    fill_rect(fb, stride, x + w - 1, y, 1, h, color);
}

unsafe fn set_url(s: &[u8]) {
    let dst = URL_BUF.as_mut_ptr();
    let len = s.len().min(60);
    let mut i = 0;
    while i < 64 {
        write_volatile(dst.add(i), 0);
        i += 1;
    }
    let mut j = 0;
    while j < len {
        unsafe fn set_url(s: &[u8]) {
            let dst = URL_BUF.as_mut_ptr();
            let src = s.as_ptr();
            let len = s.len().min(60);
            let mut i = 0;
            while i < 64 {
                write_volatile(dst.add(i), 0);
                i += 1;
            }
            let mut j = 0;
            while j < len {
                write_volatile(dst.add(j), *src.add(j));
                j += 1;
            }
            URL_LEN = len;
        }

        unsafe fn set_page_text(s: &[u8]) {
            let dst = PAGE_BUF.as_mut_ptr();
            let src = s.as_ptr();
            let len = s.len().min(4096);
            let mut i = 0;
            while i < len {
                write_volatile(dst.add(i), *src.add(i));
                i += 1;
            }
            PAGE_LEN = len;
            SCROLL_LINE = 0;
        }

    }
    URL_LEN = len;
}

unsafe fn set_page_text(s: &[u8]) {
    let dst = PAGE_BUF.as_mut_ptr();
    let len = s.len().min(4096);
    let mut i = 0;
    while i < len {
        write_volatile(dst.add(i), *s.as_ptr().add(i));
        i += 1;
    }
    PAGE_LEN = len;
    SCROLL_LINE = 0;
}

pub unsafe fn open_start_page() {
    ACTIVE_TAB = 0;
    set_url(b"miden://home");
    set_page_text(
        b"=== midenBrowser 2.0 ===\n\n\
Dobro pozhalovat v vstroennyy brauzer midenOS!\n\n\
Kak polzovatsya:\n\
1. Nazhmite na zakladku sverkhu: [example.com] ili [info.cern.ch]\n\
2. Ili kliknite po adresnoy stroke, vvedite domen i nazhmite [Pereyti]\n\
3. Ispolzuyte knopki [Tema] i prokrutku sprava dlya chteniya\n\n\
Podderzhivayutsya realnye HTTP-sayty cherez DNS + TCP/IP.",
    );
    STATUS_MSG = "Домашняя страница midenBrowser";
}

pub unsafe fn init_browser() {
    open_start_page();
}

pub unsafe fn navigate_current_url() {
    if URL_LEN == 0 {
        open_start_page();
        return;
    }
    let slice = core::slice::from_raw_parts(URL_BUF.as_ptr(), URL_LEN);
    if let Ok(raw_host) = core::str::from_utf8(slice) {
        let trimmed = raw_host.trim();
        if trimmed == "miden://home" || trimmed == "home" {
            open_start_page();
            return;
        }

        let host = trimmed
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("example.com");

        if host.is_empty() {
            STATUS_MSG = "Ошибка: пустой адрес";
            return;
        }

        if !crate::net::is_link_up() {
            STATUS_MSG = "Нет подключения к сети (проверьте кабель/QEMU)";
            set_page_text(
                b"Oshibka seti: net podklyucheniya.\n\n\
Proverte, chto QEMU zapushen s setyu (-device e1000) ili podklyuchen kabel Ethernet.",
            );
            return;
        }

        crate::net::poll_network_stack();
        let ip = crate::net::CURRENT_IP;
        if ip == [0u8, 0u8, 0u8, 0u8] {
            STATUS_MSG = "Ожидание IP от роутера (DHCP)... Нажмите [Перейти] ещё раз";
            set_page_text(
                b"Poluchenie IP-adresa po DHCP...\n\n\
Podozhdite 1-2 sekundy i nazhmite knopku [Pereyti] povtorno.",
            );
            return;
        }

        STATUS_MSG = "Загрузка страницы...";
        SCROLL_LINE = 0;
        PAGE_LEN = crate::net::http_get_page(host, &mut PAGE_BUF);
        STATUS_MSG = "Страница загружена";
    }
}

pub unsafe fn handle_browser_key(ch: u8) {
    if !WIN_BROWSER || !URL_FOCUSED {
        return;
    }
    let dst = URL_BUF.as_mut_ptr();
    if ch == 10 || ch == 13 {
        URL_FOCUSED = false;
        navigate_current_url();
    } else if ch == 8 || ch == 127 {
        if URL_LEN > 0 {
            URL_LEN -= 1;
            write_volatile(dst.add(URL_LEN), 0);
        }
    } else if ch >= 32 && ch <= 126 && URL_LEN < 58 {
        write_volatile(dst.add(URL_LEN), ch);
        URL_LEN += 1;
    }
}

pub unsafe fn draw_browser(fb: *mut u32, stride: isize, mx: isize, my: isize, focused: bool) {
    if !WIN_BROWSER {
        return;
    }
    let wx = BROWSER_X;
    let wy = BROWSER_Y;
    let ww = BROWSER_W;
    let wh = BROWSER_H;

    // Тень и корпус окна
    fill_rect(fb, stride, wx + 6, wy + 6, ww, wh, 0x00020617);
    fill_rect(fb, stride, wx, wy, ww, wh, 0x000F172A);
    draw_box_border(fb, stride, wx, wy, ww, wh, if focused { 0x0038BDF8 } else { 0x00334155 });

    // 1. Шапка окна (высота 32 px)
    fill_rect(fb, stride, wx + 1, wy + 1, ww - 2, 31, if focused { 0x001E293B } else { 0x000F172A });
    fill_rect(fb, stride, wx + 12, wy + 9, 14, 14, 0x000284C7);
    crate::kernels::kernel_64::draw_txt(fb, wx + 34, wy + 11, "midenBrowser — Интернет-обозреватель", 0x00F8FAFC);

    // Кнопка закрытия [X]
    let close_x = wx + ww - 32;
    let close_y = wy + 5;
    let close_hov = mx >= close_x && mx <= close_x + 24 && my >= close_y && my <= close_y + 22;
    fill_rect(fb, stride, close_x, close_y, 24, 22, if close_hov { 0x00EF4444 } else { 0x00991B1B });
    crate::kernels::kernel_64::draw_txt(fb, close_x + 8, close_y + 6, "X", 0x00FFFFFF);

    // 2. Панель навигации и адресная строка (wy + 32 .. wy + 68)
    fill_rect(fb, stride, wx + 1, wy + 32, ww - 2, 36, 0x00162032);

    // Кнопка "Домой" [H]
    let home_hov = mx >= wx + 10 && mx <= wx + 38 && my >= wy + 37 && my <= wy + 63;
    fill_rect(fb, stride, wx + 10, wy + 37, 28, 26, if home_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 20, wy + 45, "H", 0x0038BDF8);

    // Кнопка "Обновить" [R]
    let rel_hov = mx >= wx + 42 && mx <= wx + 70 && my >= wy + 37 && my <= wy + 63;
    fill_rect(fb, stride, wx + 42, wy + 37, 28, 26, if rel_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 52, wy + 45, "R", 0x0038BDF8);

    // Поле адресной строки
    let url_x = wx + 76;
    let url_w = 434;
    fill_rect(fb, stride, url_x, wy + 37, url_w, 26, 0x00020617);
    draw_box_border(
        fb,
        stride,
        url_x,
        wy + 37,
        url_w,
        26,
        if URL_FOCUSED { 0x0038BDF8 } else { 0x00334155 },
    );

    let ulen = URL_LEN;
    if ulen > 0 {
        let uslice = core::slice::from_raw_parts(URL_BUF.as_ptr(), ulen);
        if let Ok(ustr) = core::str::from_utf8(uslice) {
            crate::kernels::kernel_64::draw_txt(fb, url_x + 10, wy + 45, ustr, 0x00F8FAFC);
        }
    }
    if URL_FOCUSED {
        crate::kernels::kernel_64::draw_txt(fb, url_x + 10 + (ulen as isize) * 8, wy + 45, "_", 0x0038BDF8);
    }

    // Кнопка очистки [x] внутри адресной строки
    let clr_x = url_x + url_w - 24;
    let clr_hov = mx >= clr_x && mx <= clr_x + 20 && my >= wy + 39 && my <= wy + 61;
    fill_rect(fb, stride, clr_x, wy + 39, 20, 22, if clr_hov { 0x00475569 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, clr_x + 6, wy + 45, "x", 0x00CBD5E1);

    // Кнопка "Перейти"
    let go_x = wx + 518;
    let go_w = 110;
    let go_hov = mx >= go_x && mx <= go_x + go_w && my >= wy + 37 && my <= wy + 63;
    fill_rect(fb, stride, go_x, wy + 37, go_w, 26, if go_hov { 0x003B82F6 } else { 0x002563EB });
    crate::kernels::kernel_64::draw_txt(fb, go_x + 26, wy + 45, "Перейти", 0x00FFFFFF);

    // 3. Панель закладок (wy + 68 .. wy + 96)
    fill_rect(fb, stride, wx + 1, wy + 68, ww - 2, 28, 0x000F172A);
    fill_rect(fb, stride, wx + 1, wy + 95, ww - 2, 1, 0x001E293B);

    // Закладка 1: Старт
    let t0_hov = mx >= wx + 10 && mx <= wx + 82 && my >= wy + 71 && my <= wy + 92;
    fill_rect(fb, stride, wx + 10, wy + 71, 72, 21, if ACTIVE_TAB == 0 || t0_hov { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 22, wy + 76, "Главная", 0x0038BDF8);

    // Закладка 2: example.com
    let t1_hov = mx >= wx + 88 && mx <= wx + 198 && my >= wy + 71 && my <= wy + 92;
    fill_rect(fb, stride, wx + 88, wy + 71, 110, 21, if ACTIVE_TAB == 1 || t1_hov { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 98, wy + 76, "example.com", 0x0038BDF8);

    // Закладка 3: info.cern.ch
    let t2_hov = mx >= wx + 204 && mx <= wx + 318 && my >= wy + 71 && my <= wy + 92;
    fill_rect(fb, stride, wx + 204, wy + 71, 114, 21, if ACTIVE_TAB == 2 || t2_hov { 0x001E293B } else { 0x000F172A });
    crate::kernels::kernel_64::draw_txt(fb, wx + 214, wy + 76, "info.cern.ch", 0x0038BDF8);

    // Кнопка переключения темы страницы [Тема]
    let th_hov = mx >= wx + 478 && mx <= wx + 544 && my >= wy + 71 && my <= wy + 92;
    fill_rect(fb, stride, wx + 478, wy + 71, 66, 21, if th_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 492, wy + 76, "Тема", 0x00E2E8F0);

    // Кнопки прокрутки [-] и [+]
    let up_hov = mx >= wx + 550 && mx <= wx + 586 && my >= wy + 71 && my <= wy + 92;
    fill_rect(fb, stride, wx + 550, wy + 71, 36, 21, if up_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 564, wy + 76, "^", 0x00F8FAFC);
    let dn_hov = mx >= wx + 590 && mx <= wx + 626 && my >= wy + 71 && my <= wy + 92;
    fill_rect(fb, stride, wx + 590, wy + 71, 36, 21, if dn_hov { 0x00334155 } else { 0x001E293B });
    crate::kernels::kernel_64::draw_txt(fb, wx + 604, wy + 76, "v", 0x00F8FAFC);
    // 4. Область отображения веб-страницы (wy + 98 .. wy + 414)
    let page_bg = if DARK_PAGE { 0x00090D16 } else { 0x00F8FAFC };
    let page_fg = if DARK_PAGE { 0x00E2E8F0 } else { 0x000F172A };
    fill_rect(fb, stride, wx + 10, wy + 98, ww - 20, wh - 126, page_bg);
    // Построчный рендер текста с учётом прокрутки SCROLL_LINE
    let plen = PAGE_LEN;
    let p_ptr = PAGE_BUF.as_ptr();
    let mut cur_line = 0usize;
    let mut draw_y = wy + 108;
    let mut line_start = 0usize;
    let mut idx = 0usize;
    while idx <= plen && draw_y < wy + wh - 36 {
        let is_nl = idx == plen || read_volatile(p_ptr.add(idx)) == 10;
        let line_len = idx - line_start;
        if is_nl || line_len >= 72 {
            if cur_line >= SCROLL_LINE {
                if line_len > 0 {
                    let lslice = core::slice::from_raw_parts(p_ptr.add(line_start), line_len);
                    if let Ok(lstr) = core::str::from_utf8(lslice) {
                        crate::kernels::kernel_64::draw_txt(fb, wx + 20, draw_y, lstr, page_fg);
                    }
                }
                draw_y += 17;
            }
            cur_line += 1;
            line_start = if is_nl { idx + 1 } else { idx };
        }
        idx += 1;
    }
    // 5. Нижняя статусная строка
    fill_rect(fb, stride, wx + 1, wy + wh - 24, ww - 2, 23, 0x000B1120);
    crate::kernels::kernel_64::draw_txt(fb, wx + 14, wy + wh - 18, STATUS_MSG, 0x0094A3B8);
}
/// Возвращает true, если клик попал в окно браузера
pub unsafe fn handle_browser_mouse_down(
    mx: isize,
    my: isize,
    drag_win: &mut usize,
    drag_off_x: &mut isize,
    drag_off_y: &mut isize,
    focused_win: &mut usize,
) -> bool {
    if !WIN_BROWSER {
        return false;
    }
    let wx = BROWSER_X;
    let wy = BROWSER_Y;
    let ww = BROWSER_W;
    let wh = BROWSER_H;
    if mx < wx || mx > wx + ww || my < wy || my > wy + wh {
        URL_FOCUSED = false;
        return false;
    }
    *focused_win = 8;
    // 1. Кнопка закрытия [X]
    if mx >= wx + ww - 32 && mx <= wx + ww - 8 && my >= wy + 5 && my <= wy + 27 {
        WIN_BROWSER = false;
        URL_FOCUSED = false;
        return true;
    }
    // 2. Захват верхней шапки для перетаскивания окна
    if my >= wy && my <= wy + 32 {
        *drag_win = 8;
        *drag_off_x = mx - wx;
        *drag_off_y = my - wy;
        return true;
    }
    // 3. Кнопка "Домой" [H]
    if mx >= wx + 10 && mx <= wx + 38 && my >= wy + 37 && my <= wy + 63 {
        URL_FOCUSED = false;
        open_start_page();
        return true;
    }
    // 4. Кнопка "Обновить" [R]
    if mx >= wx + 42 && mx <= wx + 70 && my >= wy + 37 && my <= wy + 63 {
        URL_FOCUSED = false;
        navigate_current_url();
        return true;
    }
    // 5. Кнопка очистки [x] в адресной строке
    let url_x = wx + 76;
    let url_w = 434;
    let clr_x = url_x + url_w - 24;
    if mx >= clr_x && mx <= clr_x + 20 && my >= wy + 39 && my <= wy + 61 {
        set_url(b"");
        URL_FOCUSED = true;
        STATUS_MSG = "Введите адрес сайта и нажмите Enter";
        return true;
    }
    // 6. Поле адресной строки
    if mx >= url_x && mx <= url_x + url_w && my >= wy + 37 && my <= wy + 63 {
        URL_FOCUSED = true;
        STATUS_MSG = "Ввод адреса • Нажмите Enter для перехода";
        return true;
    }
    URL_FOCUSED = false;
    // 7. Кнопка "Перейти"
    if mx >= wx + 518 && mx <= wx + 628 && my >= wy + 37 && my <= wy + 63 {
        navigate_current_url();
        return true;
    }
    // 8. Закладки
    if mx >= wx + 10 && mx <= wx + 82 && my >= wy + 71 && my <= wy + 92 {
        open_start_page();
        return true;
    }
    if mx >= wx + 88 && mx <= wx + 198 && my >= wy + 71 && my <= wy + 92 {
        ACTIVE_TAB = 1;
        set_url(b"example.com");
        navigate_current_url();
        return true;
    }
    if mx >= wx + 204 && mx <= wx + 318 && my >= wy + 71 && my <= wy + 92 {
        ACTIVE_TAB = 2;
        set_url(b"info.cern.ch");
        navigate_current_url();
        return true;
    }
    // 9. Переключение темы страницы
    if mx >= wx + 478 && mx <= wx + 544 && my >= wy + 71 && my <= wy + 92 {
        DARK_PAGE = !DARK_PAGE;
        return true;
    }
    // 10. Прокрутка [^] и [v]
    if mx >= wx + 550 && mx <= wx + 586 && my >= wy + 71 && my <= wy + 92 {
        SCROLL_LINE = SCROLL_LINE.saturating_sub(3);
        return true;
    }
    if mx >= wx + 590 && mx <= wx + 626 && my >= wy + 71 && my <= wy + 92 {
        SCROLL_LINE = SCROLL_LINE.saturating_add(3);
        return true;
    }
    true
}

