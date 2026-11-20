#!/bin/bash
set -e

echo "=== 1. ТОТАЛЬНОЕ УНИЧТОЖЕНИЕ КЭША И СТАРЫХ БИЛДОВ ==="
rm -f boot.o boot.bin kernel.bin midenOS.iso librust_kernel.a
rm -rf iso_root
if [ -d "kernel" ]; then
    cd kernel
    cargo clean
    cd ..
fi

echo "=== 2. СБОРКА ЧИСТОГО Rust ЯДРА С НУЛЯ ==="
cd kernel
cargo build --release --target x86_64-unknown-none
cd ..

echo "=== 3. КОПИРОВАНИЕ НОВОЙ БИБЛИОТЕКИ ==="
cp kernel/target/x86_64-unknown-none/release/libkernel.a ./librust_kernel.a
echo "✅ Новая статическая библиотека готова!"

echo "=== 4. АСЕМБЛИРОВАНИЕ ЗАГРУЗЧИКА ==="
nasm -f elf64 boot.asm -o boot.o

echo "=== 5. ЖЕСТКАЯ ЛИНКОВКА МОНОЛИТА ==="
ld -m elf_x86_64 -s -z max-page-size=0x1000 --no-warn-rwx-segments -T linker.ld \
   boot.o \
   --whole-archive ./librust_kernel.a -no-whole-archive \
   -o kernel.bin

echo "=== 6. ПОДГОТОВКА СВЕЖЕГО ISO КАТАЛОГА ==="
mkdir -p iso_root/boot/grub
cp kernel.bin iso_root/boot/kernel.bin

cat << 'EOG' > iso_root/boot/grub/grub.cfg
set timeout=5
set default=0
insmod all_video
insmod vbe
insmod efi_gop
insmod gfxterm
set gfxmode=1024x768x32
set gfxpayload=keep

menuentry "midenOS 64-bit" {
    multiboot2 /boot/kernel.bin entry_64bit
    boot
}

menuentry "midenOS Diagnostics (Miledin)" {
    multiboot2 /boot/kernel.bin diag
    boot
}
EOG

echo "=== 7. СОЗДАНИЕ ОБРАЗА ==="
grub-mkrescue -o midenOS.iso iso_root

echo "=== 8. ЗАПУСК ОБНОВЛЕННОЙ СИСТЕМЫ В QEMU ==="
qemu-system-x86_64 \
    -m 512M \
    -vga std \
    -cdrom midenOS.iso \
    -audio none \
    -no-reboot \
    -no-shutdown
