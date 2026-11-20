#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

static mut CPU_HISTORY: [u8; 48] = [12; 48];
static mut LAST_TSC: u64 = 0;
static mut SAMPLE_TICK: u32 = 0;
pub static mut LAST_BENCH_SCORE: u32 = 0;

#[inline(always)]
unsafe fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    ((hi as u64) << 32) | (lo as u64)
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

pub unsafe fn sample_cpu_activity() {
    SAMPLE_TICK = SAMPLE_TICK.wrapping_add(1);
    if SAMPLE_TICK % 8 != 0 {
        return;
    }
    let now = rdtsc();
    let delta = now.wrapping_sub(LAST_TSC);
    LAST_TSC = now;

    let load = (((delta >> 14) & 0x3F) as u8).clamp(8, 92);

    let ptr = CPU_HISTORY.as_mut_ptr();
    let mut i = 0;
    while i < 47 {
        let next = read_volatile(ptr.add(i + 1));
        write_volatile(ptr.add(i), next);
        i += 1;
    }
    write_volatile(ptr.add(47), load);
}

pub unsafe fn run_cpu_benchmark() -> u32 {
    let start = rdtsc();
    let mut primes = 0u32;
    let mut n = 2u32;
    while n < 12_000 {
        let mut is_p = true;
        let mut d = 2u32;
        while d * d <= n {
            if n % d == 0 {
                is_p = false;
                break;
            }
            d += 1;
        }
        if is_p {
            primes += 1;
        }
        n += 1;
    }
    let elapsed = rdtsc().wrapping_sub(start) >> 12;
    let score = if elapsed > 0 {
        (primes.wrapping_mul(1000) / (elapsed as u32)).clamp(120, 9999)
    } else {
        2500
    };
    LAST_BENCH_SCORE = score;
    score
}

unsafe fn format_u32(mut val: u32, buf: &mut [u8]) -> usize {
    let dst = buf.as_mut_ptr();
    if val == 0 {
        write_volatile(dst, b'0');
        return 1;
    }
    let mut tmp = [0u8; 12];
    let t_ptr = tmp.as_mut_ptr();
    let mut len = 0usize;
    while val > 0 && len < 12 {
        write_volatile(t_ptr.add(len), b'0' + ((val % 10) as u8));
        val /= 10;
        len += 1;
    }
    let mut i = 0;
    while i < len {
        let ch = read_volatile(t_ptr.add(len - 1 - i));
        write_volatile(dst.add(i), ch);
        i += 1;
    }
    len
}

pub unsafe fn draw_sysmon_widget(fb: *mut u32, stride: isize, wx: isize, wy: isize, mx: isize, my: isize) {
    let gx = wx + 16;
    let gy = wy + 174;
    let gw = 340;
    let gh = 78;

    fill_rect(fb, stride, gx, gy, gw, gh, 0x00020617);
    fill_rect(fb, stride, gx, gy, gw, 1, 0x00334155);
    fill_rect(fb, stride, gx, gy + gh - 1, gw, 1, 0x00334155);

    let ptr = CPU_HISTORY.as_ptr();
    let mut i = 0isize;
    while i < 48 {
        let val = read_volatile(ptr.add(i as usize)) as isize;
        let bh = (val * (gh - 8)) / 100;
        let col = if val > 75 {
            0x00EF4444
        } else if val > 45 {
            0x00FACC15
        } else {
            0x0038BDF8
        };
        fill_rect(fb, stride, gx + 4 + i * 7, gy + gh - 4 - bh, 5, bh, col);
        i += 1;
    }

    let used_kb = (crate::HEAP_OFFSET.load(core::sync::atomic::Ordering::Relaxed) / 1024) as u32;
    let mut mem_line = *b"Heap RAM: 0000 / 4096 KB";
    let mut num_buf = [0u8; 12];
    let nlen = format_u32(used_kb, &mut num_buf);
    let m_ptr = mem_line.as_mut_ptr();
    let n_ptr = num_buf.as_ptr();
    let mut k = 0;
    while k < 4 {
        let ch = if k < nlen { read_volatile(n_ptr.add(k)) } else { b' ' };
        write_volatile(m_ptr.add(10 + k), ch);
        k += 1;
    }
    if let Ok(mstr) = core::str::from_utf8(&mem_line) {
        crate::kernels::kernel_64::draw_txt(fb, gx, gy + gh + 10, mstr, 0x00CBD5E1);
    }

    let bx = gx + 220;
    let by = gy + gh + 6;
    let bhov = mx >= bx && mx <= bx + 120 && my >= by && my <= by + 24;
    fill_rect(fb, stride, bx, by, 120, 24, if bhov { 0x002563EB } else { 0x001D4ED8 });
    crate::kernels::kernel_64::draw_txt(fb, bx + 26, by + 7, "Тест CPU", 0x00FFFFFF);

    if LAST_BENCH_SCORE > 0 {
        let mut sc_line = *b"CPU Score: 0000 pts";
        let slen = format_u32(LAST_BENCH_SCORE, &mut num_buf);
        let s_ptr = sc_line.as_mut_ptr();
        let mut j = 0;
        while j < 4 {
            let ch = if j < slen { read_volatile(n_ptr.add(j)) } else { b' ' };
            write_volatile(s_ptr.add(11 + j), ch);
            j += 1;
        }
        if let Ok(sstr) = core::str::from_utf8(&sc_line) {
            crate::kernels::kernel_64::draw_txt(fb, gx, gy + gh + 32, sstr, 0x004ADE80);
        }
    }
}
