; mbr_loader.s — Загрузчик MidenOS (Медведь, XP Bar, поиск 32-bit VBE 1024x768x32 и чистый рабочий стол)
[BITS 16]
[ORG 0x7C00]

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    mov [boot_drive], dl

    ; Читаем Stage 2 (4 сектора с LBA 1 -> 0x0000:0x7E00)
    mov si, dap_stage2
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    jc disk_error

    jmp 0x0000:0x7E00

disk_error:
    mov ah, 0x0E
    mov al, 'E'
    int 0x10
    hlt
    jmp $

boot_drive: db 0x80

align 4
dap_stage2:
    db 0x10
    db 0x00
    dw 4
    dw 0x7E00
    dw 0x0000
    dq 1

times 510 - ($ - $$) db 0
dw 0xAA55

; ==============================================================================
; STAGE 2 (LBA 1..4, адрес 0x7E00)
; ==============================================================================
stage2_start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    sti

    ; 1. Поиск настоящего 32-битного режима VBE 1024x768x32 (BitsPerPixel == 32)
    mov word [vbe_mode], 0x0118     ; Запасной вариант
    mov ax, 0x4F00
    mov di, 0x0600
    mov dword [es:di], 'VBE2'
    int 0x10
    cmp ax, 0x004F
    jne .set_vbe

    ; Копируем список номеров режимов VBE в безопасный буфер 0x0700 (чтобы не менять сегменты в цикле)
    mov si, [0x060E]                ; offset списка режимов
    mov bx, [0x0610]                ; segment списка режимов
    mov di, 0x0700
    mov cx, 64
.copy_modes:
    mov ds, bx
    mov ax, [si]
    xor dx, dx
    mov ds, dx
    mov [di], ax
    cmp ax, 0xFFFF
    je .scan_copied_modes
    add si, 2
    add di, 2
    dec cx
    jnz .copy_modes
    mov word [di], 0xFFFF

.scan_copied_modes:
    mov si, 0x0700
.mode_loop:
    mov cx, [si]
    cmp cx, 0xFFFF
    je .set_vbe
    add si, 2

    push si
    xor ax, ax
    mov es, ax
    mov ax, 0x4F01
    mov di, 0x0500
    int 0x10
    pop si
    cmp ax, 0x004F
    jne .mode_loop

    ; Проверяем поддержку Linear Framebuffer (bit 7), 1024x768 и 32 бита на пиксель!
    test byte [0x0500], 0x80
    jz .mode_loop
    cmp word [0x0512], 1024
    jne .mode_loop
    cmp word [0x0514], 768
    jne .mode_loop
    cmp byte [0x0519], 32           ; Строго 32-битный цвет (4 байта на пиксель)!
    jne .mode_loop

    mov [vbe_mode], cx

.set_vbe:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ax, 0x4F01
    mov cx, [vbe_mode]
    mov di, 0x0500
    int 0x10

    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ax, 0x4F02
    mov bx, [vbe_mode]
    or bx, 0x4000                   ; Включаем Linear Framebuffer
    int 0x10

    xor ax, ax
    mov ds, ax
    mov es, ax

    mov eax, [0x0528]               ; VBE PhysBasePtr
    mov [fb_addr], eax
    movzx eax, word [0x0510]        ; BytesPerScanLine
    mov [fb_pitch], eax
    movzx eax, byte [0x0519]        ; BitsPerPixel
    mov [fb_bpp], eax
    shr eax, 3
    mov [fb_bypp], eax

    ; 2. Включаем A20
    in al, 0x92
    or al, 2
    and al, 0xFE
    out 0x92, al

    ; 3. Читаем splash.bmp (LBA 16, 760 секторов = 380 КБ) в RAM 0x02000000 (32 МБ)
    mov edi, 0x02000000
    mov dword [dap_lba], 16
    mov cx, 760
.read_bmp:
    push cx
    push edi
    xor ax, ax
    mov ds, ax
    mov si, dap_io
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    pop edi
    pop cx
    jc .read_kernel_start

    call enable_unreal_fs

    mov esi, 0x00010000
    mov dx, 128
.copy_bmp:
    mov eax, [fs:esi]
    mov [fs:edi], eax
    add esi, 4
    add edi, 4
    dec dx
    jnz .copy_bmp

    inc dword [dap_lba]
    dec cx
    jnz .read_bmp

    ; 4. Читаем ядро (LBA 2048, 10600 секторов) в RAM 0x00100000
.read_kernel_start:
    mov edi, 0x00100000
    mov dword [dap_lba], 2048
    mov cx, 10600
.read_kern:
    push cx
    push edi
    xor ax, ax
    mov ds, ax
    mov si, dap_io
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    pop edi
    pop cx
    jc disk_error

    call enable_unreal_fs

    mov esi, 0x00010000
    mov dx, 128
.copy_kern:
    mov eax, [fs:esi]
    mov [fs:edi], eax
    add esi, 4
    add edi, 4
    dec dx
    jnz .copy_kern

    inc dword [dap_lba]
    dec cx
    jnz .read_kern

    ; 5. Переходим в 32-bit Protected Mode
    cli
    lgdt [gdtr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    jmp 0x08:pm32_main

enable_unreal_fs:
    cli
    lgdt [gdtr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    mov bx, 0x10
    mov fs, bx
    and al, 0xFE
    mov cr0, eax
    xor ax, ax
    mov ds, ax
    mov es, ax
    sti
    ret

vbe_mode: dw 0x0118
fb_addr:  dd 0
fb_pitch: dd 0
fb_bpp:   dd 32
fb_bypp:  dd 4

align 4
dap_io:
    db 0x10
    db 0x00
    dw 1
    dw 0x0000
    dw 0x1000
dap_lba:
    dq 0

align 8
gdt_start:
    dq 0x0000000000000000           ; 0x00: Null
    dq 0x00CF9A000000FFFF           ; 0x08: 32-bit Code
    dq 0x00CF92000000FFFF           ; 0x10: 32-bit Data
gdt_end:

gdtr:
    dw gdt_end - gdt_start - 1
    dd gdt_start

; ==============================================================================
; 32-БИТНЫЙ РЕЖИМ: МЕДВЕДЬ ПО ЦЕНТРУ + XP BAR + ЗАПУСК BOOT.ASM (0x100030)
; ==============================================================================
[BITS 32]
pm32_main:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, 0x00007C00

    ; 1. Очищаем экран в чёрный (#000000)
    mov edi, [fb_addr]
    mov ebx, 768
.cls_y:
    mov ecx, [fb_pitch]
    xor eax, eax
    push edi
    rep stosb
    pop edi
    add edi, [fb_pitch]
    dec ebx
    jnz .cls_y

    ; 2. Рисуем splash.bmp из 0x02000000 строго по центру
    mov esi, 0x02000000
    cmp word [esi], 0x4D42
    jne .draw_xp_frame

    mov eax, [esi + 0x0A]
    add eax, 0x02000000
    mov [bmp_data], eax

    mov eax, [esi + 0x12]
    mov [bmp_w], eax
    mov eax, [esi + 0x16]
    mov [bmp_h], eax
    movzx eax, word [esi + 0x1C]
    shr eax, 3
    mov [bmp_bytes], eax

    imul eax, [bmp_w]
    add eax, 3
    and eax, 0xFFFFFFFC
    mov [bmp_stride], eax

    mov eax, [bmp_w]
    cmp eax, 400
    jle .w_ok
    mov eax, 400
.w_ok:
    mov [draw_w], eax

    mov eax, [bmp_h]
    cmp eax, 400
    jle .h_ok
    mov eax, 400
.h_ok:
    mov [draw_h], eax

    mov eax, 1024
    sub eax, [draw_w]
    shr eax, 1
    mov [off_x], eax

    mov eax, 768
    sub eax, [draw_h]
    shr eax, 1
    sub eax, 40
    mov [off_y], eax

    xor ebx, ebx
.bmp_y_loop:
    cmp ebx, [draw_h]
    jge .draw_xp_frame

    mov eax, [bmp_h]
    dec eax
    sub eax, ebx
    imul eax, [bmp_stride]
    add eax, [bmp_data]
    mov esi, eax

    mov eax, [off_y]
    add eax, ebx
    imul eax, [fb_pitch]
    add eax, [fb_addr]
    mov edx, [off_x]
    imul edx, [fb_bypp]
    add eax, edx
    mov edi, eax

    xor ecx, ecx
.bmp_x_loop:
    cmp ecx, [draw_w]
    jge .bmp_next_y

    mov al, [esi]
    mov [edi], al
    mov al, [esi + 1]
    mov [edi + 1], al
    mov al, [esi + 2]
    mov [edi + 2], al

    add esi, [bmp_bytes]
    add edi, [fb_bypp]
    inc ecx
    jmp .bmp_x_loop

.bmp_next_y:
    inc ebx
    jmp .bmp_y_loop

    ; 3. Рамка прогресс-бара Windows XP по центру (X=402, Y=555)
.draw_xp_frame:
    xor ebx, ebx
.fr_y:
    cmp ebx, 18
    jge .xp_anim
    mov eax, ebx
    add eax, 555
    imul eax, [fb_pitch]
    add eax, [fb_addr]
    mov edx, 402
    imul edx, [fb_bypp]
    add eax, edx
    mov edi, eax

    xor ecx, ecx
.fr_x:
    cmp ecx, 220
    jge .fr_next_y
    xor eax, eax
    cmp ebx, 0
    je .fr_gray
    cmp ebx, 17
    je .fr_gray
    cmp ecx, 0
    je .fr_gray
    cmp ecx, 219
    je .fr_gray
    jmp .fr_put
.fr_gray:
    mov eax, 0x008A8F99
.fr_put:
    mov [edi], al
    shr eax, 8
    mov [edi + 1], al
    shr eax, 8
    mov [edi + 2], al
    add edi, [fb_bypp]
    inc ecx
    jmp .fr_x
.fr_next_y:
    inc ebx
    jmp .fr_y

    ; 4. Анимация бегущих синих кубиков XP (2 прохода)
.xp_anim:
    mov dword [anim_pass], 2
.pass_loop:
    mov dword [bar_x], 0
.step_loop:
    xor ebx, ebx
.clr_in_y:
    cmp ebx, 10
    jge .draw_3_blocks
    mov eax, ebx
    add eax, 559
    imul eax, [fb_pitch]
    add eax, [fb_addr]
    mov edx, 406
    imul edx, [fb_bypp]
    add eax, edx
    mov edi, eax

    mov ecx, 212
.clr_in_x:
    mov byte [edi], 0
    mov byte [edi + 1], 0
    mov byte [edi + 2], 0
    add edi, [fb_bypp]
    dec ecx
    jnz .clr_in_x

    inc ebx
    jmp .clr_in_y

.draw_3_blocks:
    xor esi, esi
.b_loop:
    cmp esi, 3
    jge .step_delay
    mov edx, esi
    imul edx, 16
    add edx, [bar_x]

    xor ebx, ebx
.b_y:
    cmp ebx, 10
    jge .b_next
    mov eax, ebx
    add eax, 559
    imul eax, [fb_pitch]
    add eax, [fb_addr]
    mov ecx, 406
    imul ecx, [fb_bypp]
    add eax, ecx
    mov edi, eax

    xor ecx, ecx
.b_x:
    cmp ecx, 12
    jge .b_next_y
    mov eax, edx
    add eax, ecx
    cmp eax, 212
    jge .b_skip

    push edi
    imul eax, [fb_bypp]
    add edi, eax
    mov byte [edi], 0xF6
    mov byte [edi + 1], 0x82
    mov byte [edi + 2], 0x3B
    pop edi

.b_skip:
    inc ecx
    jmp .b_x
.b_next_y:
    inc ebx
    jmp .b_y
.b_next:
    inc esi
    jmp .b_loop

.step_delay:
    mov ecx, 8000000
.delay_loop:
    pause
    dec ecx
    jnz .delay_loop

    add dword [bar_x], 6
    cmp dword [bar_x], 168
    jl .step_loop

    dec dword [anim_pass]
    jnz .pass_loop

    ; 5. Формируем Multiboot2 Boot Info по адресу 0x9000
    mov dword [0x9000], 48
    mov dword [0x9004], 0
    mov dword [0x9008], 8
    mov dword [0x900C], 32
    mov eax, [fb_addr]
    mov [0x9010], eax
    mov dword [0x9014], 0
    mov eax, [fb_pitch]
    mov [0x9018], eax
    mov dword [0x901C], 1024
    mov dword [0x9020], 768
    mov eax, [fb_bpp]
    or eax, 0x00000100
    mov [0x9024], eax
    mov dword [0x9028], 0
    mov dword [0x902C], 8

    ; 6. Переход в boot.asm (0x00100030)
    cli
    mov eax, 0x36D76289             ; Multiboot2 Magic
    mov ebx, 0x00009000             ; Multiboot2 Info
    mov ecx, 0x00100030             ; start в boot.asm
    jmp ecx

bmp_data:   dd 0
bmp_w:      dd 0
bmp_h:      dd 0
bmp_bytes:  dd 0
bmp_stride: dd 0
draw_w:     dd 0
draw_h:     dd 0
off_x:      dd 0
off_y:      dd 0
bar_x:      dd 0
anim_pass:  dd 0
times 2560 - ($ - $$) db 0
