#![allow(dead_code)]

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

#[derive(Copy, Clone, PartialEq)]
pub enum DriveKind {
    None,
    AtaHardDisk,
    AtapiOptical,
}

#[derive(Copy, Clone, PartialEq)]
pub enum OpticalTrayState {
    NoDrive,
    TrayOpenOrEmpty,
    MediaReady,
    MediaJustInserted,
}

pub static mut DETECTED_KIND: DriveKind = DriveKind::None;
pub static mut ACTIVE_BASE: u16 = 0;
pub static mut ACTIVE_SLAVE: bool = false;
pub static mut TRAY_STATE: OpticalTrayState = OpticalTrayState::NoDrive;
pub static mut LAST_DISC_LABEL: [u8; 32] = [0; 32];
pub static mut LAST_DISC_LABEL_LEN: usize = 0;

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

#[inline(always)]
unsafe fn inw(port: u16) -> u16 {
    let val: u16;
    asm!("in ax, dx", out("ax") val, in("dx") port, options(nomem, nostack));
    val
}

#[inline(always)]
unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack));
}

unsafe fn io_wait(base: u16) {
    let _ = inb(base + 7);
    let _ = inb(base + 7);
    let _ = inb(base + 7);
    let _ = inb(base + 7);
}

unsafe fn wait_not_busy(base: u16) -> bool {
    let mut timeout = 500_000u32;
    while timeout > 0 {
        let st = inb(base + 7);
        if (st & 0x80) == 0 {
            return true;
        }
        timeout -= 1;
    }
    false
}

unsafe fn wait_drq(base: u16) -> bool {
    let mut timeout = 500_000u32;
    while timeout > 0 {
        let st = inb(base + 7);
        if (st & 0x01) != 0 {
            return false; // Error bit
        }
        if (st & 0x80) == 0 && (st & 0x08) != 0 {
            return true; // DRQ ready
        }
        timeout -= 1;
    }
    false
}

/// Сканирует каналы IDE (Primary 0x1F0, Secondary 0x170) и ищет CD/DVD-привод или HDD
pub unsafe fn init_disk_driver() -> DriveKind {
    DETECTED_KIND = DriveKind::None;
    ACTIVE_BASE = 0;
    ACTIVE_SLAVE = false;
    TRAY_STATE = OpticalTrayState::NoDrive;

    let channels: [(u16, bool); 4] = [
        (0x1F0, false),
        (0x1F0, true),
        (0x170, false),
        (0x170, true),
    ];

    let ch_ptr = channels.as_ptr();
    let mut idx = 0usize;
    while idx < 4 {
        let (base, slave) = read_volatile(ch_ptr.add(idx));
        outb(base + 6, if slave { 0xB0 } else { 0xA0 });
        io_wait(base);

        outb(base + 2, 0);
        outb(base + 3, 0);
        outb(base + 4, 0);
        outb(base + 5, 0);
        outb(base + 7, 0xEC); // ATA IDENTIFY
        io_wait(base);

        let st = inb(base + 7);
        if st != 0 && st != 0xFF {
            let lba_mid = inb(base + 4);
            let lba_hi = inb(base + 5);

            // Сигнатура ATAPI CD/DVD-привода: 0x14 и 0xEB
            if lba_mid == 0x14 && lba_hi == 0xEB {
                DETECTED_KIND = DriveKind::AtapiOptical;
                ACTIVE_BASE = base;
                ACTIVE_SLAVE = slave;
                TRAY_STATE = OpticalTrayState::TrayOpenOrEmpty;
                let _ = poll_optical_drive();
                return DETECTED_KIND;
            } else if lba_mid == 0 && lba_hi == 0 && DETECTED_KIND == DriveKind::None {
                // Обычный жёсткий диск ATA (запоминаем, но продолжаем искать оптический привод)
                if wait_drq(base) {
                    let mut w = 0;
                    while w < 256 {
                        let _ = inw(base);
                        w += 1;
                    }
                    DETECTED_KIND = DriveKind::AtaHardDisk;
                    ACTIVE_BASE = base;
                    ACTIVE_SLAVE = slave;
                }
            }
        }
        idx += 1;
    }

    DETECTED_KIND
}

/// Отправляет 12-байтный SCSI-пакет в ATAPI CD/DVD привод
unsafe fn send_atapi_packet(packet: &[u8; 12], byte_count: u16) -> bool {
    if DETECTED_KIND != DriveKind::AtapiOptical || ACTIVE_BASE == 0 {
        return false;
    }
    let base = ACTIVE_BASE;
    outb(base + 6, if ACTIVE_SLAVE { 0xB0 } else { 0xA0 });
    io_wait(base);

    if !wait_not_busy(base) {
        return false;
    }

    outb(base + 1, 0x00); // Features = PIO
    outb(base + 4, (byte_count & 0xFF) as u8);
    outb(base + 5, ((byte_count >> 8) & 0xFF) as u8);
    outb(base + 7, 0xA0); // PACKET command
    io_wait(base);

    if !wait_drq(base) {
        return false;
    }

    let p_ptr = packet.as_ptr() as *const u16;
    let mut i = 0usize;
    while i < 6 {
        outw(base, read_volatile(p_ptr.add(i)));
        i += 1;
    }
    true
}

/// Проверяет состояние дисковода: открыт ли лоток или вставлен новый диск
pub unsafe fn poll_optical_drive() -> OpticalTrayState {
    if DETECTED_KIND != DriveKind::AtapiOptical {
        TRAY_STATE = OpticalTrayState::NoDrive;
        return TRAY_STATE;
    }

    // SCSI команда TEST UNIT READY (0x00)
    let tur_cmd: [u8; 12] = [0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let ok = send_atapi_packet(&tur_cmd, 0) && wait_not_busy(ACTIVE_BASE);
    let st = inb(ACTIVE_BASE + 7);
    let err = inb(ACTIVE_BASE + 1);

    let prev = TRAY_STATE;
    if ok && (st & 0x01) == 0 {
        if prev != OpticalTrayState::MediaReady && prev != OpticalTrayState::MediaJustInserted {
            TRAY_STATE = OpticalTrayState::MediaJustInserted;
            read_iso9660_volume_label();
        } else {
            TRAY_STATE = OpticalTrayState::MediaReady;
        }
    } else {
        // Sense Key 6 (Unit Attention) означает, что диск только что вставили/закрыли лоток
        let sense_key = (err >> 4) & 0x0F;
        if sense_key == 0x06 {
            TRAY_STATE = OpticalTrayState::MediaJustInserted;
            read_iso9660_volume_label();
        } else {
            TRAY_STATE = OpticalTrayState::TrayOpenOrEmpty;
            LAST_DISC_LABEL_LEN = 0;
        }
    }

    TRAY_STATE
}

/// Программно открыть или закрыть лоток дисковода (eject = true -> открыть, false -> закрыть)
pub unsafe fn eject_optical_tray(eject: bool) -> bool {
    if DETECTED_KIND != DriveKind::AtapiOptical {
        return false;
    }
    // SCSI START STOP UNIT (0x1B): byte 4 = 0x02 (Eject), 0x03 (Load/Close)
    let flag = if eject { 0x02 } else { 0x03 };
    let cmd: [u8; 12] = [0x1B, 0, 0, 0, flag, 0, 0, 0, 0, 0, 0, 0];
    let res = send_atapi_packet(&cmd, 0);
    let _ = wait_not_busy(ACTIVE_BASE);
    if eject {
        TRAY_STATE = OpticalTrayState::TrayOpenOrEmpty;
        LAST_DISC_LABEL_LEN = 0;
    }
    res
}

/// Читает один сектор (2048 байт) с CD/DVD диска по адресу LBA
pub unsafe fn read_cd_sector(lba: u32, out_buf: &mut [u8; 2048]) -> bool {
    if DETECTED_KIND != DriveKind::AtapiOptical {
        return false;
    }
    // SCSI READ (12) (0xA8), 1 сектор (2048 байт)
    let cmd: [u8; 12] = [
        0xA8,
        0,
        ((lba >> 24) & 0xFF) as u8,
        ((lba >> 16) & 0xFF) as u8,
        ((lba >> 8) & 0xFF) as u8,
        (lba & 0xFF) as u8,
        0,
        0,
        0,
        1, // 1 sector
        0,
        0,
    ];

    if !send_atapi_packet(&cmd, 2048) {
        return false;
    }
    if !wait_drq(ACTIVE_BASE) {
        return false;
    }

    let base = ACTIVE_BASE;
    let dst16 = out_buf.as_mut_ptr() as *mut u16;
    let mut i = 0usize;
    while i < 1024 {
        write_volatile(dst16.add(i), inw(base));
        i += 1;
    }
    let _ = wait_not_busy(base);
    true
}

/// Читает метку тома ISO9660 (сектор 16) при вставке диска
unsafe fn read_iso9660_volume_label() {
    let mut sector = [0u8; 2048];
    if !read_cd_sector(16, &mut sector) {
        return;
    }
    let s_ptr = sector.as_ptr();
    // Сигнатура "CD001" со смещения 1..6
    if read_volatile(s_ptr.add(1)) == b'C'
        && read_volatile(s_ptr.add(2)) == b'D'
        && read_volatile(s_ptr.add(3)) == b'0'
        && read_volatile(s_ptr.add(4)) == b'0'
        && read_volatile(s_ptr.add(5)) == b'1'
        {
            let lbl_dst = LAST_DISC_LABEL.as_mut_ptr();
            let mut len = 0usize;
            let mut i = 0usize;
            while i < 31 {
                let ch = read_volatile(s_ptr.add(40 + i));
                write_volatile(lbl_dst.add(i), ch);
                if ch != b' ' && ch != 0 {
                    len = i + 1;
                }
                i += 1;
            }
            LAST_DISC_LABEL_LEN = len;
        }
}

/// Читает список файлов из корневого каталога ISO9660 в буфер out_text
pub unsafe fn list_cd_files(out_text: &mut [u8]) -> usize {
    let mut pvd = [0u8; 2048];
    if !read_cd_sector(16, &mut pvd) {
        return 0;
    }
    let p_ptr = pvd.as_ptr();
    // Корневая запись каталога в PVD находится по смещению 156; её LBA — смещение 156 + 2 = 158 (little-endian)
    let root_lba = (read_volatile(p_ptr.add(158)) as u32)
    | ((read_volatile(p_ptr.add(159)) as u32) << 8)
    | ((read_volatile(p_ptr.add(160)) as u32) << 16)
    | ((read_volatile(p_ptr.add(161)) as u32) << 24);

    let mut dir_sec = [0u8; 2048];
    if !read_cd_sector(root_lba, &mut dir_sec) {
        return 0;
    }

    let d_ptr = dir_sec.as_ptr();
    let dst = out_text.as_mut_ptr();
    let mut off = 0usize;
    let mut written = 0usize;

    while off < 2048 {
        let rec_len = read_volatile(d_ptr.add(off)) as usize;
        if rec_len == 0 {
            break;
        }
        let name_len = read_volatile(d_ptr.add(off + 32)) as usize;
        // Пропускаем служебные записи "." (0x00) и ".." (0x01)
        if name_len > 1 && off + 33 + name_len <= 2048 {
            let mut k = 0usize;
            while k < name_len && written + 2 < out_text.len() {
                let ch = read_volatile(d_ptr.add(off + 33 + k));
                if ch == b';' {
                    break; // Отбрасываем суффикс версии ISO9660 ";1"
                }
                write_volatile(dst.add(written), ch);
                written += 1;
                k += 1;
            }
            if written + 1 < out_text.len() {
                write_volatile(dst.add(written), 10); // '\n'
                written += 1;
            }
        }
        off += rec_len;
    }
    written
}
/// Ищет файл по имени на CD/DVD диске и читает его содержимое в out_data
pub unsafe fn read_cd_file(target_name: &str, out_data: &mut [u8]) -> Option<usize> {
    let mut pvd = [0u8; 2048];
    if !read_cd_sector(16, &mut pvd) {
        return None;
    }
    let p_ptr = pvd.as_ptr();
    let root_lba = (read_volatile(p_ptr.add(158)) as u32)
    | ((read_volatile(p_ptr.add(159)) as u32) << 8)
    | ((read_volatile(p_ptr.add(160)) as u32) << 16)
    | ((read_volatile(p_ptr.add(161)) as u32) << 24);
    let mut dir_sec = [0u8; 2048];
    if !read_cd_sector(root_lba, &mut dir_sec) {
        return None;
    }
    let d_ptr = dir_sec.as_ptr();
    let mut off = 0usize;
    while off < 2048 {
        let rec_len = read_volatile(d_ptr.add(off)) as usize;
        if rec_len == 0 {
            break;
        }
        let name_len = read_volatile(d_ptr.add(off + 32)) as usize;
        if name_len > 1 && off + 33 + name_len <= 2048 {
            let mut name_buf = [0u8; 64];
            let nb_ptr = name_buf.as_mut_ptr();
            let mut clean_len = 0usize;
            while clean_len < name_len && clean_len < 63 {
                let ch = read_volatile(d_ptr.add(off + 33 + clean_len));
                if ch == b';' {
                    break;
                }
                write_volatile(nb_ptr.add(clean_len), ch);
                clean_len += 1;
            }
            let slice = core::slice::from_raw_parts(nb_ptr, clean_len);
            if let Ok(fname) = core::str::from_utf8(slice) {
                if fname.eq_ignore_ascii_case(target_name.trim()) {
                    let file_lba = (read_volatile(d_ptr.add(off + 2)) as u32)
                    | ((read_volatile(d_ptr.add(off + 3)) as u32) << 8)
                    | ((read_volatile(d_ptr.add(off + 4)) as u32) << 16)
                    | ((read_volatile(d_ptr.add(off + 5)) as u32) << 24);
                    let file_size = (read_volatile(d_ptr.add(off + 10)) as u32)
                    | ((read_volatile(d_ptr.add(off + 11)) as u32) << 8)
                    | ((read_volatile(d_ptr.add(off + 12)) as u32) << 16)
                    | ((read_volatile(d_ptr.add(off + 13)) as u32) << 24);
                    let to_read = (file_size as usize).min(out_data.len());
                    let sectors = (to_read + 2047) / 2048;
                    let dst = out_data.as_mut_ptr();
                    let mut copied = 0usize;
                    let mut s = 0usize;
                    while s < sectors {
                        let mut sec_buf = [0u8; 2048];
                        if !read_cd_sector(file_lba + (s as u32), &mut sec_buf) {
                            break;
                        }
                        let chunk = (to_read - copied).min(2048);
                        let src = sec_buf.as_ptr();
                        let mut b = 0usize;
                        while b < chunk {
                            write_volatile(dst.add(copied + b), read_volatile(src.add(b)));
                            b += 1;
                        }
                        copied += chunk;
                        s += 1;
                    }
                    return Some(copied);
                }
            }
        }
        off += rec_len;
    }
    None
}
