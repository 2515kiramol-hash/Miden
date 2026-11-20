ENTRY(start)

SECTIONS {
    /* Задаем стартовый адрес ядра для Multiboot (обычно 1 Мегабайт) */
    . = 1M;

    .text : {
        /* ЖЕСТКИЙ ФИКС: Принудительно суем секцию .multiboot на самый старт бинарника! */
        KEEP(*(.multiboot))
        *(.text)
    }

    .rodata : {
        *(.rodata)
    }

    .data : {
        *(.data)
    }

    .bss : {
        *(.bss)
    }
}

