#![allow(dead_code)]

use core::ptr::{read_volatile, write_volatile};

pub unsafe fn build_sysinf_lines(out: &mut [[u8; 76]; 11], uptime_ticks: u64) {
    let art: [&[u8]; 11] = [
        b"    (\\_.-''''-._/)      OS: midenOS 64-bit Fluent Edition",
        b"   /   __    __   \\     Kernel: Native Rust x86_64 (Ring 0)",
        b"  (   (o )  ( o)   )    Uptime: 00:00:00",
        b"   \\      /\\      /     CPU: x86_64 @ Hardware TSC",
        b"  .-'\\   '--'   /'-.    Memory: 0000 / 4096 KB Heap",
        b" / o  )========(    \\   Display: 1024x768 (32-bit VBE)",
        b"(o_o / \\    )  Shell: midenVFS Terminal",
        b" `-' |  [  M ]  |   /   Mascot: Baity (Misha)",
        b"     |   ||||   |__/    User: Administrator",
        b"     \\__________/       Theme: Fluent Glass 60 FPS",
        b"       |__||__|         Status: Ring-0 Operational",
    ];

    let art_ptr = art.as_ptr();
    let out_ptr = out.as_mut_ptr();

    let mut row = 0usize;
    while row < 11 {
        let row_buf = (*out_ptr.add(row)).as_mut_ptr();
        let mut c = 0usize;
        while c < 76 {
            write_volatile(row_buf.add(c), 0);
            c += 1;
        }

        let src_slice = read_volatile(art_ptr.add(row));
        let src_ptr = src_slice.as_ptr();
        let len = src_slice.len().min(75);
        let mut i = 0usize;
        while i < len {
            write_volatile(row_buf.add(i), read_volatile(src_ptr.add(i)));
            i += 1;
        }
        row += 1;
    }

    let total_sec = (uptime_ticks / 60) as u32;
    let hrs = (total_sec / 3600) % 100;
    let mins = (total_sec / 60) % 60;
    let secs = total_sec % 60;

    let r2 = (*out_ptr.add(2)).as_mut_ptr();
    write_volatile(r2.add(32), b'0' + ((hrs / 10) as u8));
    write_volatile(r2.add(33), b'0' + ((hrs % 10) as u8));
    write_volatile(r2.add(35), b'0' + ((mins / 10) as u8));
    write_volatile(r2.add(36), b'0' + ((mins % 10) as u8));
    write_volatile(r2.add(38), b'0' + ((secs / 10) as u8));
    write_volatile(r2.add(39), b'0' + ((secs % 10) as u8));

    let used_kb = (crate::HEAP_OFFSET.load(core::sync::atomic::Ordering::Relaxed) / 1024) as u32;
    let r4 = (*out_ptr.add(4)).as_mut_ptr();
    write_volatile(r4.add(32), b'0' + (((used_kb / 1000) % 10) as u8));
    write_volatile(r4.add(33), b'0' + (((used_kb / 100) % 10) as u8));
    write_volatile(r4.add(34), b'0' + (((used_kb / 10) % 10) as u8));
    write_volatile(r4.add(35), b'0' + ((used_kb % 10) as u8));
}
