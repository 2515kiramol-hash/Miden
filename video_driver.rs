use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};

static GPU_BAR0: AtomicUsize = AtomicUsize::new(0);

extern "C" {
    fn nvidia_c_init() -> i32;
}

unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack));
}

unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    asm!("in eax, dx", out("eax") val, in("dx") port, options(nomem, nostack));
    val
}

unsafe fn pci_read_u32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(0xCF8, addr);
    inl(0xCFC)
}

pub fn init() -> bool {
    for bus in 0..=255u8 {
        for slot in 0..32u8 {
            let id = unsafe { pci_read_u32(bus, slot, 0, 0x00) };
            let vendor = (id & 0xFFFF) as u16;
            if vendor == 0x10DE {
                let raw0 = unsafe { pci_read_u32(bus, slot, 0, 0x10) };
                let is_64bit = (raw0 & 0x06) == 0x04;
                let bar0_low = (raw0 & 0xFFFF_FFF0) as usize;
                let bar0 = if is_64bit {
                    let bar0_high = unsafe { pci_read_u32(bus, slot, 0, 0x14) } as usize;
                    (bar0_high << 32) | bar0_low
                } else {
                    bar0_low
                };

                if bar0 != 0 {
                    GPU_BAR0.store(bar0, Ordering::SeqCst);
                    let res = unsafe { nvidia_c_init() };
                    return res == 0;
                }
            }
        }
    }
    false
}

#[no_mangle]
pub unsafe extern "C" fn c_nv_rd32(offset: u32) -> u32 {
    let base = GPU_BAR0.load(Ordering::SeqCst);
    if base == 0 {
        return 0;
    }
    core::ptr::read_volatile((base + offset as usize) as *const u32)
}

#[no_mangle]
pub unsafe extern "C" fn c_nv_wr32(offset: u32, val: u32) {
    let base = GPU_BAR0.load(Ordering::SeqCst);
    if base == 0 {
        return;
    }
    core::ptr::write_volatile((base + offset as usize) as *mut u32, val);
}
