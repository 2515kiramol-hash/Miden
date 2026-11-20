// kernel/src/mbr.rs — Запись MBR (LBA 0), Stage-2 (LBA 1..4) и splash.bmp (LBA 16+) на диск
static MBR_BIN: &[u8] = include_bytes!("../../mbr.bin");
static SPLASH_BMP: &[u8] = include_bytes!("splash.bmp");

pub unsafe fn install_mbr_and_splash<F>(
    total_disk_sectors: u64,
    mut write_sector: F,
) -> bool
where
F: FnMut(u64, *const u8) -> bool,
{
    if MBR_BIN.len() < 2560 {
        return false;
    }

    // 1. Формируем сектор LBA 0 (код из mbr.bin + таблица разделов + 0x55AA)
    let mut mbr = [0u8; 512];
    for i in 0..440 {
        *mbr.get_unchecked_mut(i) = *MBR_BIN.get_unchecked(i);
    }

    *mbr.get_unchecked_mut(440) = 0x4D;
    *mbr.get_unchecked_mut(441) = 0x49;
    *mbr.get_unchecked_mut(442) = 0x44;
    *mbr.get_unchecked_mut(443) = 0x4E;

    // Активный раздел с LBA 2048
    *mbr.get_unchecked_mut(446) = 0x80;
    *mbr.get_unchecked_mut(447) = 0x20;
    *mbr.get_unchecked_mut(448) = 0x21;
    *mbr.get_unchecked_mut(449) = 0x00;
    *mbr.get_unchecked_mut(450) = 0x83;
    *mbr.get_unchecked_mut(451) = 0xFE;
    *mbr.get_unchecked_mut(452) = 0xFF;
    *mbr.get_unchecked_mut(453) = 0xFF;

    // Start LBA = 2048 (0x00000800)
    *mbr.get_unchecked_mut(454) = 0x00;
    *mbr.get_unchecked_mut(455) = 0x08;
    *mbr.get_unchecked_mut(456) = 0x00;
    *mbr.get_unchecked_mut(457) = 0x00;

    let part_sectors: u32 = if total_disk_sectors > 2048 {
        (total_disk_sectors - 2048).min(0xFFFF_FFFF) as u32
    } else {
        131072
    };
    *mbr.get_unchecked_mut(458) = (part_sectors & 0xFF) as u8;
    *mbr.get_unchecked_mut(459) = ((part_sectors >> 8) & 0xFF) as u8;
    *mbr.get_unchecked_mut(460) = ((part_sectors >> 16) & 0xFF) as u8;
    *mbr.get_unchecked_mut(461) = ((part_sectors >> 24) & 0xFF) as u8;

    *mbr.get_unchecked_mut(510) = 0x55;
    *mbr.get_unchecked_mut(511) = 0xAA;

    if !write_sector(0, mbr.as_ptr()) {
        return false;
    }

    // 2. Записываем 4 сектора Stage-2 (LBA 1..4)
    for sec in 0..4u64 {
        let offset = 512 + (sec as usize) * 512;
        if !write_sector(1 + sec, MBR_BIN.as_ptr().add(offset)) {
            return false;
        }
    }

    // 3. Записываем splash.bmp на диск начиная с LBA 16 (до 1040 секторов = 520 КБ)
    let bmp_sectors = ((SPLASH_BMP.len() + 511) / 512).min(1040);
    for s in 0..bmp_sectors {
        let mut sec_buf = [0u8; 512];
        let start = s * 512;
        let end = (start + 512).min(SPLASH_BMP.len());
        for i in start..end {
            *sec_buf.get_unchecked_mut(i - start) = *SPLASH_BMP.get_unchecked(i);
        }
        if !write_sector(16 + (s as u64), sec_buf.as_ptr()) {
            return false;
        }
    }

    true
}
