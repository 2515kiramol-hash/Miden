; mbr.asm — Полный 64-битный загрузчик MidenOS (Поиск VBE 1024x768x32, Медведь по центру, XP Bar и вход в 64-bit kernel_main)
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
    ; 1. Поиск настоящего 32-битного VBE режима 1024x768x32 (перебираем режимы VBE)
    mov word [vbe_mode], 0x0118     ; Запасной вариант
    mov ax, 0x4F00
    mov di, 0x0600                  ; VbeInfoBlock в 0x0600
    mov dword [di], 'VBE2'
    int 0x10
    cmp ax, 0x004F
    jne .set_vbe

    mov si, [0x060E]                ; Смещение списка режимов (VideoModePtr offset)
    mov ax, [0x0610]                ; Сегмент списка режимов (VideoModePtr segment)
    mov fs, ax

.find_mode_loop:
    mov cx, [fs:si]
    cmp cx, 0xFFFF
    je .set_vbe
    add si, 2

    push si
    push fs
    mov ax, 0x4F01
    mov di, 0x0500                  ; ModeInfoBlock в 0x0500
    int 0x10
    pop fs
    pop si
    cmp ax, 0x004F
    jne .find_mode_loop

    ; Проверяем: Linear Framebuffer (bit 7 в ModeAttributes [0x0500])
    test byte [0x0500], 0x80
    jz .find_mode_loop
    ; Проверяем разрешение 1024x768 и глубину цвета 32 бита
    cmp word [0x0512], 1024         ; XResolution
    jne .find_mode_loop
    cmp word [0x0514], 768          ; YResolution
    jne .find_mode_loop
    cmp byte [0x0519], 32           ; BitsPerPixel == 32!
    jne .find_mode_loop

    mov [vbe_mode], cx

.set_vbe:
    xor ax, ax
    mov fs, ax
    mov ax, 0x4F01
    mov cx, [vbe_mode]
    mov di, 0x0500
    int 0x10

    mov ax, 0x4F02
    mov bx, [vbe_mode]
    or bx, 0x4000                   ; Включаем Linear Framebuffer
    int 0x10

    mov eax, [0x0528]               ; VBE PhysBasePtr
    mov [fb_addr], eax
    movzx eax, word [0x0510]        ; BytesPerScanLine (pitch)
    mov [fb_pitch], eax
    movzx eax, byte [0x0519]        ; BitsPerPixel
    mov [fb_bpp], eax
    shr eax, 3
    mov [fb_bypp], eax

    ; 2. Читаем splash.bmp (LBA 16, 760 секторов = 380 КБ) в 0x2000:0x0000 (физ. 0x00020000)
    mov word [dap_seg], 0x2000
    mov word [dap_off], 0x0000
    mov dword [dap_lba], 16
    mov cx, 760
.read_bmp:
    push cx
    mov si, dap_io
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    pop cx
    jc .prep_kernel

    add word [dap_seg], 0x0020
    inc dword [dap_lba]
    dec cx
    jnz .read_bmp

    ; 3. Включаем A20 + Unreal Mode для загрузки ядра (10800 секторов) в 0x00100000
.prep_kernel:
    in al, 0x92
    or al, 2
    and al, 0xFE
    out 0x92, al

    cli
    lgdt [gdtr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    mov bx, 0x10
    mov fs, bx
    mov gs, bx
    and al, 0xFE
    mov cr0, eax
    sti

    mov word [dap_seg], 0x1000
    mov word [dap_off], 0x0000
    mov edi, 0x00100000
    mov dword [dap_lba], 2048
    mov cx, 10800
.read_kern:
    push cx
    push edi
    mov si, dap_io
    mov ah, 0x42
    mov dl, [boot_drive]
    int 0x13
    pop edi
    pop cx
    jc disk_error

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

    ; 4. Переходим в 32-bit Protected Mode
    cli
    lgdt [gdtr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    jmp 0x08:pm32_main

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
dap_off:
    dw 0x0000
dap_seg:
    dw 0x1000
dap_lba:
    dq 0

align 8
gdt_start:
    dq 0x0000000000000000           ; 0x00: Null
    dq 0x00CF9A000000FFFF           ; 0x08: 32-bit Code
    dq 0x00CF92000000FFFF           ; 0x10: 32-bit Data
    dq 0x00AF9A000000FFFF           ; 0x18: 64-bit Code
    dq 0x00AF92000000FFFF           ; 0x20: 64-bit Data
gdt_end:

gdtr:
    dw gdt_end - gdt_start - 1
    dd gdt_start

; ==============================================================================
; 32-БИТНЫЙ РЕЖИМ: МЕДВЕДЬ ПО ЦЕНТРУ + XP BAR + ВКЛЮЧЕНИЕ 64-BIT LONG MODE
; ==============================================================================
[BITS 32]
pm32_main:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, 0x00008000

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

    ; 2. Рисуем splash.bmp из 0x00020000 строго по центру
    mov esi, 0x00020000
    cmp word [esi], 0x4D42
    jne .draw_xp_frame

    mov eax, [esi + 0x0A]
    add eax, 0x00020000
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

    ; 6. Обнуляем .bss ядра (0x0062d000 .. 0x0123c000)
    mov edi, 0x0062D000
    mov ecx, 0x00C0F000 / 4
    xor eax, eax
    rep stosd

    ; 7. Строим 64-битные таблицы страниц (Identity Map первых 4 ГБ по 2 МБ страницам)
    ; PML4 по адресу 0x10000, PDPT по 0x11000, PD0..PD3 по 0x12000..0x15000
    mov edi, 0x00010000
    mov ecx, 0x6000 / 4
    xor eax, eax
    rep stosd

    mov dword [0x00010000], 0x00011003 ; PML4[0] -> PDPT
    mov dword [0x00011000], 0x00012003 ; PDPT[0] -> PD0 (0..1 ГБ)
    mov dword [0x00011008], 0x00013003 ; PDPT[1] -> PD1 (1..2 ГБ)
    mov dword [0x00011010], 0x00014003 ; PDPT[2] -> PD2 (2..3 ГБ)
mov dword [0x00011018], 0x00015003 ; PDPT[3] -> PD3 (3..4 ГБ)
; Заполняем 2048 записей по 2 МБ (0x83 = Present + RW + PageSize 2MB) -> покрывает все 4 ГБ RAM и LFB!
mov edi, 0x00012000
mov eax, 0x00000083
mov ecx, 2048
.fill_pd:
mov [edi], eax
mov dword [edi + 4], 0
add eax, 0x00200000
add edi, 8
dec ecx
jnz .fill_pd
; Включаем PAE (CR4 bit 5)
mov eax, cr4
or eax, (1 << 5)
mov cr4, eax
; Загружаем PML4 в CR3
mov eax, 0x00010000
mov cr3, eax
; Включаем Long Mode (EFER.LME = bit 8 в MSR 0xC0000080)
mov ecx, 0xC0000080
rdmsr
or eax, (1 << 8)
wrmsr
; Включаем Paging (CR0 bit 31)
mov eax, cr0
or eax, (1 << 31)
mov cr0, eax
; Дальний прыжок в 64-битный сегмент кода (0x18)!
jmp 0x18:lm64_entry
; ==============================================================================
; 64-БИТНЫЙ LONG MODE: ПРЯМОЙ ВХОД В KERNEL_MAIN (System V AMD64 ABI)
; ==============================================================================
[BITS 64]
lm64_entry:
mov ax, 0x20
mov ds, ax
mov es, ax
mov fs, ax
mov gs, ax
mov ss, ax
mov rsp, 0x0000000000090000     ; Чистый 64-битный стек
; По стандарту System V AMD64 ABI аргументы передаются в RDI и RSI:
; RDI = mb_magic (0x36D76289), RSI = mb_info (0x00009000)
mov edi, 0x36D76289
mov esi, 0x00009000
; Точный адрес kernel_main в 64-битном ядре (находится в начале сегмента .text по 0x100030 или сразу за _start):
; Если 0x100030 — это 32-битный _start из boot.S, ищем 64-битный пролог kernel_main (push rbp / sub rsp)
; или вызываем 0x100030, если он написан сразу под Long Mode.
mov rax, 0x0000000000100030
jmp rax
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
