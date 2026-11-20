// kernel/src/paint_app.rs — Графический редактор Paint для midenOS
use crate::kernels::kernel_64::{draw_txt, SCREEN_W};

pub const PAINT_W: isize = 540;
pub const PAINT_H: isize = 380;
const CANVAS_W: usize = 516;
const CANVAS_H: usize = 290;

static mut CANVAS: [u32; CANVAS_W * CANVAS_H] = [0x00FFFFFF; CANVAS_W * CANVAS_H];
static mut CUR_COLOR: u32 = 0x001E293B;
static mut BRUSH_SIZE: isize = 3;
static mut PREV_X: isize = -1;
static mut PREV_Y: isize = -1;

const PALETTE: [u32; 10] = [
  0x000F172A, // Чёрный
0x00FFFFFF, // Белый (ластик)
0x00EF4444, // Красный
0x00F97316, // Оранжевый
0x00EAB308, // Жёлтый
0x0022C55E, // Зелёный
0x0006B6D4, // Голубой
0x003B82F6, // Синий
0x008B5CF6, // Фиолетовый
0x00EC4899, // Розовый
];

unsafe fn put_brush(cx: isize, cy: isize) {
  let r = BRUSH_SIZE;
  for dy in -r..=r {
    for dx in -r..=r {
      if dx * dx + dy * dy <= r * r {
        let px = cx + dx;
        let py = cy + dy;
        if px >= 0 && px < CANVAS_W as isize && py >= 0 && py < CANVAS_H as isize {
          let idx = (py as usize) * CANVAS_W + (px as usize);
          *CANVAS.get_unchecked_mut(idx) = CUR_COLOR;
        }
      }
    }
  }
}

pub unsafe fn handle_paint_mouse(wx: isize, wy: isize, mx: isize, my: isize, pressed: bool) -> bool {
  if !pressed {
    PREV_X = -1;
    PREV_Y = -1;
    return false;
  }

  // Проверка палитры цветов (Y: wy + 38 .. wy + 62)
  if my >= wy + 38 && my <= wy + 62 {
    for i in 0..10 {
      let bx = wx + 12 + (i as isize) * 28;
      if mx >= bx && mx <= bx + 22 {
        CUR_COLOR = PALETTE[i];
        return true;
      }
    }
    // Выбор размера кисти (S / M / L)
    if mx >= wx + 310 && mx <= wx + 342 { BRUSH_SIZE = 2; return true; }
    if mx >= wx + 348 && mx <= wx + 380 { BRUSH_SIZE = 4; return true; }
    if mx >= wx + 386 && mx <= wx + 418 { BRUSH_SIZE = 7; return true; }
    // Кнопка «Очистить»
    if mx >= wx + 436 && mx <= wx + 526 {
      for px in CANVAS.iter_mut() { *px = 0x00FFFFFF; }
      return true;
    }
  }

  // Рисование на холсте (wx + 12 .. wx + 528, wy + 74 .. wy + 364)
  let cx = mx - (wx + 12);
  let cy = my - (wy + 74);
  if cx >= 0 && cx < CANVAS_W as isize && cy >= 0 && cy < CANVAS_H as isize {
    if PREV_X >= 0 && PREV_Y >= 0 {
      let steps = (cx - PREV_X).abs().max((cy - PREV_Y).abs()).max(1);
      for s in 0..=steps {
        let ix = PREV_X + ((cx - PREV_X) * s) / steps;
        let iy = PREV_Y + ((cy - PREV_Y) * s) / steps;
        put_brush(ix, iy);
      }
    } else {
      put_brush(cx, cy);
    }
    PREV_X = cx;
    PREV_Y = cy;
    return true;
  }
  false
}

pub unsafe fn draw_paint_window(fb: *mut u32, wx: isize, wy: isize) {
  // Панель инструментов
  for dy in 32..68 {
    let row = fb.offset((wy + dy) * SCREEN_W);
    for dx in 4..(PAINT_W - 4) {
      *row.offset(wx + dx) = 0x001E293B;
    }
  }

  // Квадраты палитры
  for (i, &col) in PALETTE.iter().enumerate() {
    let bx = wx + 12 + (i as isize) * 28;
    let by = wy + 39;
    for dy in 0..22 {
      let row = fb.offset((by + dy) * SCREEN_W);
      for dx in 0..22 {
        let is_sel = col == CUR_COLOR && (dx == 0 || dx == 21 || dy == 0 || dy == 21);
        *row.offset(bx + dx) = if is_sel { 0x0038BDF8 } else { col };
      }
    }
  }

  // Кнопки размера кисти и очистки
  draw_txt(fb, wx + 314, wy + 44, "[S]", if BRUSH_SIZE == 2 { 0x0038BDF8 } else { 0x00CBD5E1 });
  draw_txt(fb, wx + 352, wy + 44, "[M]", if BRUSH_SIZE == 4 { 0x0038BDF8 } else { 0x00CBD5E1 });
  draw_txt(fb, wx + 390, wy + 44, "[L]", if BRUSH_SIZE == 7 { 0x0038BDF8 } else { 0x00CBD5E1 });
  draw_txt(fb, wx + 442, wy + 44, "[Очистить]", 0x00F87171);

  // Отрисовка холста
  for y in 0..CANVAS_H {
    let dst = fb.offset((wy + 74 + y as isize) * SCREEN_W + (wx + 12));
    let src = CANVAS.as_ptr().add(y * CANVAS_W);
    core::ptr::copy_nonoverlapping(src, dst, CANVAS_W);
  }
}
