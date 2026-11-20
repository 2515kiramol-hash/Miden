section .multiboot
align 8
mb_header_start:
    dd 0xE85250D6                   ; Multiboot2 magic
    dd 0                            ; Architecture: i386 (protected mode)
    dd mb_header_end - mb_header_start
    dd -(0xE85250D6 + 0 + (mb_header_end - mb_header_start))

    ; Framebuffer tag (1024x768x32)
    align 8
    dw 5
    dw 0
    dd 20
    dd 1024
    dd 768
    dd 32

    ; End tag
    align 8
    dw 0
    dw 0
    dd 8
mb_header_end:

section .bss
align 4096
p4_table:
    resb 4096
p3_table:
    resb 4096
p2_tables:
    resb 4096 * 4

align 16
stack_bottom:
    resb 65536
stack_top:

mb_info_ptr:
    resd 1
m_fb_addr:
    resd 1
m_fb_addr_hi:
    resd 1
m_fb_pitch:
    resd 1
m_fb_width:
    resd 1
m_fb_height:
    resd 1
m_fb_bpp:
    resb 1
m_diag_flag:
    resb 1
m_install_flag:
    resb 1
align 4

section .rodata
gdt64:
    dq 0
.code: equ $ - gdt64
    dq (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)
.data: equ $ - gdt64
    dq (1 << 44) | (1 << 47) | (1 << 41)
.pointer:
    dw $ - gdt64 - 1
    dq gdt64

section .text
[BITS 32]
global start
extern kernel_main

start:
    cli
    cld
    mov esp, stack_top

    ; Если загрузка идёт с флешки (EBX != 0x9000), сохраняем чистый эталон ядра в 0x02000000
    cmp ebx, 0x9000
    je .skip_backup
    mov esi, 0x00100000
    mov edi, 0x02000000
    mov ecx, (10600 * 512) / 4
    rep movsd
.skip_backup:

    mov [mb_info_ptr], ebx
    mov dword [m_fb_addr], 0
    mov dword [m_fb_addr_hi], 0
    mov dword [m_fb_pitch], 1024 * 4
    mov dword [m_fb_width], 1024
    mov dword [m_fb_height], 768
    mov byte [m_fb_bpp], 32
    mov byte [m_diag_flag], 0
    mov byte [m_install_flag], 0

    ; Разбираем теги Multiboot2
    mov esi, ebx
    test esi, esi
    jz .tags_done
    add esi, 8
.mb_tag_loop:
    mov eax, [esi]
    mov ecx, [esi + 4]
    cmp eax, 0
    je .tags_done
    cmp ecx, 8
    jl .tags_done

    cmp eax, 8
    jne .not_fb
    mov edx, [esi + 8]
    mov [m_fb_addr], edx
    mov edx, [esi + 12]
    mov [m_fb_addr_hi], edx
    mov edx, [esi + 16]
    test edx, edx
    jz .skip_pitch
    mov [m_fb_pitch], edx
.skip_pitch:
    mov edx, [esi + 20]
    mov [m_fb_width], edx
    mov edx, [esi + 24]
    mov [m_fb_height], edx
    mov dl, [esi + 28]
    mov [m_fb_bpp], dl
    jmp .next_tag

.not_fb:
    cmp eax, 1
    jne .next_tag
    lea edi, [esi + 8]
    mov edx, ecx
    sub edx, 8
.scan_diag:
    cmp edx, 4
    jl .next_tag
    cmp dword [edi], "diag"
    je .found_diag
    inc edi
    dec edx
    jmp .scan_diag
.found_diag:
    mov byte [m_diag_flag], 1

.next_tag:
    add ecx, 7
    and ecx, ~7
    add esi, ecx
    jmp .mb_tag_loop

.tags_done:
    cmp byte [m_diag_flag], 1
    je miledin_entry
    jmp miledin_done_to_os

%include "miledin.asm"

miledin_done_to_os:
    cli
    mov esp, stack_top
    call set_up_page_tables
    call enable_long_mode
    mov ebp, [mb_info_ptr]
    lgdt [gdt64.pointer]
    jmp gdt64.code:long_mode_start

set_up_page_tables:
    mov edi, p4_table
    xor eax, eax
    mov ecx, 6144
    rep stosd

    mov eax, p3_table
    or eax, 0b11
    mov [p4_table], eax

    mov ecx, 0
.map_p3_loop:
    mov eax, p2_tables
    mov edx, ecx
    shl edx, 12
    add eax, edx
    or eax, 0b11
    mov [p3_table + ecx * 8], eax
    inc ecx
    cmp ecx, 4
    jne .map_p3_loop

    mov ecx, 0
.map_p2_loop:
    mov eax, 0x200000
    mul ecx
    or eax, 0b10000011
    mov [p2_tables + ecx * 8], eax
    inc ecx
    cmp ecx, 2048
    jne .map_p2_loop
    ret

enable_long_mode:
    mov eax, p4_table
    mov cr3, eax

    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax
    ret

[BITS 64]
long_mode_start:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov rsp, stack_top
    and rsp, -16

    ; Включаем SSE/XMM (EM=0, TS=0, MP=1)
    mov rax, cr0
    and ax, 0xFFF3
    or ax, 0x2
    mov cr0, rax

    mov rax, cr4
    or ax, (1 << 9) | (1 << 10)
    mov cr4, rax

    mov edi, 0x36D76289             ; 1-й аргумент: magic Multiboot2
    mov esi, ebp                    ; 2-й аргумент: mb_info_ptr
    movzx edx, byte [m_install_flag]; 3-й аргумент: 1 = запустить 64-bit инсталлер
    call kernel_main

.hang:
    cli
    hlt
    jmp .hang
