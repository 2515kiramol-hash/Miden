#!/bin/bash
set -e

echo "=== 1. Очистка старых объектных модулей ==="
rm -f boot.o boot.bin fresh_ui.o cs_bridge.cpp kernel.bin midenOS.iso cpp_utils.o wallpaper_raw.o midenOS.img fresh_ui.o ui.o librust_kernel.a e1000.o
rm -rf iso_root
cd kernel && cargo clean && cd ..

echo "=== 2. Сборка нативного ядра на RUST (Чистый x86_64) ==="
cd kernel
cargo build --release --target x86_64-unknown-none
cd ..

echo "=== 3. Подготовка библиотеки ядра ==="
cp kernel/target/x86_64-unknown-none/release/libkernel.a ./librust_kernel.a
echo "✅ Код ядра успешно подготовлен!"

echo "=== 4. Компиляция Assembly Загрузчика ==="
nasm -f elf64 boot.asm -o boot.o

echo "=== 4.5. Компиляция C-драйвера e1000 ==="
gcc -c -ffreestanding -fno-stack-protector -mno-red-zone -fno-pic \
    -mcmodel=kernel -O2 -o e1000.o kernel/src/kernels/e1000.c
echo "✅ C-драйвер e1000 скомпилирован!"

echo "=== 5. Чистая Линковка воедино по скрипту linker.ld ==="
ld -m elf_x86_64 -s -z max-page-size=0x1000 --no-warn-rwx-segments -T linker.ld \
    boot.o \
    e1000.o \
    --whole-archive ./librust_kernel.a -no-whole-archive \
    -o kernel.bin

echo "=== 6. Создание структуры папок ISO ==="
mkdir -p iso_root/boot/grub
cp kernel.bin iso_root/boot/

cat << 'EOG' > iso_root/boot/grub/grub.cfg
set timeout=10
set default=0

menuentry "1. midenOS 64-bit (Stable Rust Interactive Desktop)" {
    multiboot2 /boot/kernel.bin entry_64bit
    boot
}

menuentry "2. midenOS 64-bit (Diagnostic Mode / Real Hardware Check)" {
    multiboot2 /boot/kernel.bin diag
    boot
}
EOG

echo "=== 7. Генерация легкого midenOS.iso ==="
grub-mkrescue -o midenOS.iso iso_root
echo "✅ КЛИКАБЕЛЬНАЯ ОС УСПЕШНО СКОМПИЛИРОВАНА!"

echo "=== 8. Запуск в QEMU ==="
qemu-system-x86_64 -vga std -accel tcg -cdrom midenOS.iso -no-reboot -no-shutdown -device e1000
