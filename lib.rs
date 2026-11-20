#![no_std]
#![allow(static_mut_refs)]

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

pub mod video_driver;
pub mod calendar_app;
pub mod clock_app;
pub mod todo_app;
pub mod sticky_notes;
pub mod hex_editor;
pub mod weather_app;
pub mod snip_app;
pub mod power;
pub mod rdm;
pub mod sound;
pub mod sysinf;
pub mod sysmon;
pub mod mines_app;
pub mod calc_app;
pub mod cd_popup;
pub mod disk;
pub mod explorer_app;
pub mod installer;
pub mod kernels;
pub mod mbr;
pub mod notepad_app;
pub mod paint_app;
pub mod terminal_app;

struct KernelAllocator;

static mut HEAP_MEM: [u8; 4 * 1024 * 1024] = [0; 4 * 1024 * 1024];
pub static HEAP_OFFSET: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(8);
        let size = layout.size();
        loop {
            let cur = HEAP_OFFSET.load(Ordering::Relaxed);
            let aligned = (cur + align - 1) & !(align - 1);
            let next = aligned + size;
            if next > HEAP_MEM.len() {
                return core::ptr::null_mut();
            }
            if HEAP_OFFSET
                .compare_exchange(cur, next, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
                {
                    return HEAP_MEM.as_mut_ptr().add(aligned);
                }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static GLOBAL_ALLOC: KernelAllocator = KernelAllocator;

#[no_mangle]
pub extern "C" fn kernel_main(mb_magic: u32, mb_info: u32, install_flag: u32) -> ! {
    unsafe {
        if install_flag == 1 && mb_info != 0x9000 {
            crate::installer::run_installer(mb_info);
        }
        crate::kernels::kernel_64::run_graphics(mb_magic, mb_info);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn c_kmalloc(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    let layout = Layout::from_size_align_unchecked(size, 8);
    GLOBAL_ALLOC.alloc(layout)
}

#[no_mangle]
pub unsafe extern "C" fn c_kfree(_ptr: *mut u8) {}
