#include <stdint.h>
#include <stddef.h>

static inline void outl(uint16_t port, uint32_t value) {
    __asm__ volatile("outl %0, %1" : : "a"(value), "Nd"(port));
}
static inline uint32_t inl(uint16_t port) {
    uint32_t value;
    __asm__ volatile("inl %1, %0" : "=a"(value) : "Nd"(port));
    return value;
}

static uint32_t pci_read(uint8_t bus, uint8_t slot, uint8_t func, uint8_t off) {
    uint32_t addr = 0x80000000u | ((uint32_t)bus << 16) | ((uint32_t)slot << 11)
    | ((uint32_t)func << 8) | (off & 0xFC);
    outl(0xCF8, addr);
    return inl(0xCFC);
}
static void pci_write(uint8_t bus, uint8_t slot, uint8_t func, uint8_t off, uint32_t val) {
    uint32_t addr = 0x80000000u | ((uint32_t)bus << 16) | ((uint32_t)slot << 11)
    | ((uint32_t)func << 8) | (off & 0xFC);
    outl(0xCF8, addr);
    outl(0xCFC, val);
}

static volatile uint32_t *e1000_mmio = 0;
static uint8_t e1000_mac[6] = {0};

#define REG_CTRL   0x0000
#define REG_RCTL   0x0100
#define REG_TCTL   0x0400
#define REG_TIPG   0x0410
#define REG_RDBAL  0x2800
#define REG_RDBAH  0x2804
#define REG_RDLEN  0x2808
#define REG_RDH    0x2810
#define REG_RDT    0x2818
#define REG_TDBAL  0x3800
#define REG_TDBAH  0x3804
#define REG_TDLEN  0x3808
#define REG_TDH    0x3810
#define REG_TDT    0x3818
#define REG_RAL    0x5400
#define REG_RAH    0x5404

static inline void mmio_write(uint32_t reg, uint32_t val) {
    *(volatile uint32_t*)((uintptr_t)e1000_mmio + reg) = val;
}
static inline uint32_t mmio_read(uint32_t reg) {
    return *(volatile uint32_t*)((uintptr_t)e1000_mmio + reg);
}

#define NUM_TX 16
#define NUM_RX 16
#define BUF_SIZE 2048

typedef struct {
    uint64_t addr;
    uint16_t length;
    uint8_t  cso;
    uint8_t  cmd;
    uint8_t  status;
    uint8_t  css;
    uint16_t special;
} __attribute__((packed)) tx_desc_t;

typedef struct {
    uint64_t addr;
    uint16_t length;
    uint16_t checksum;
    uint8_t  status;
    uint8_t  errors;
    uint16_t special;
} __attribute__((packed)) rx_desc_t;

static tx_desc_t tx_ring[NUM_TX] __attribute__((aligned(16)));
static rx_desc_t rx_ring[NUM_RX] __attribute__((aligned(16)));
static uint8_t tx_bufs[NUM_TX][BUF_SIZE] __attribute__((aligned(16)));
static uint8_t rx_bufs[NUM_RX][BUF_SIZE] __attribute__((aligned(16)));

static uint32_t tx_cur = 0;
static uint32_t rx_cur = 0;

int e1000_init(void) {
    for (uint8_t bus = 0; bus < 8; bus++) {
        for (uint8_t slot = 0; slot < 32; slot++) {
            uint32_t vd = pci_read(bus, slot, 0, 0x00);
            if ((vd & 0xFFFF) == 0xFFFF) continue;
            if ((vd & 0xFFFF) == 0x8086 && (vd >> 16) == 0x100E) {
                uint32_t cmd = pci_read(bus, slot, 0, 0x04);
                pci_write(bus, slot, 0, 0x04, cmd | 0x6);
                uint32_t bar0 = pci_read(bus, slot, 0, 0x10) & ~0xF;
                e1000_mmio = (volatile uint32_t*)(uintptr_t)bar0;
                mmio_write(REG_CTRL, mmio_read(REG_CTRL) | (1 << 26));
                while (mmio_read(REG_CTRL) & (1 << 26)) {}
                uint32_t ral = mmio_read(REG_RAL);
                uint32_t rah = mmio_read(REG_RAH);
                e1000_mac[0] = (ral >> 0)  & 0xFF;
                e1000_mac[1] = (ral >> 8)  & 0xFF;
                e1000_mac[2] = (ral >> 16) & 0xFF;
                e1000_mac[3] = (ral >> 24) & 0xFF;
                e1000_mac[4] = (rah >> 0)  & 0xFF;
                e1000_mac[5] = (rah >> 8)  & 0xFF;
                for (int i = 0; i < NUM_TX; i++) {
                    tx_ring[i].addr = (uint64_t)(uintptr_t)tx_bufs[i];
                    tx_ring[i].status = 1;
                }
                for (int i = 0; i < NUM_RX; i++) {
                    rx_ring[i].addr = (uint64_t)(uintptr_t)rx_bufs[i];
                    rx_ring[i].status = 0;
                }
                mmio_write(REG_TDBAL, (uint32_t)(uintptr_t)tx_ring);
                mmio_write(REG_TDBAH, 0);
                mmio_write(REG_TDLEN, sizeof(tx_ring));
                mmio_write(REG_TDH, 0);
                mmio_write(REG_TDT, 0);
                mmio_write(REG_TCTL, (1<<1)|(1<<3)|(0x10<<4)|(0x40<<12));
                mmio_write(REG_TIPG, 0x0060200A);
                mmio_write(REG_RDBAL, (uint32_t)(uintptr_t)rx_ring);
                mmio_write(REG_RDBAH, 0);
                mmio_write(REG_RDLEN, sizeof(rx_ring));
                mmio_write(REG_RDH, 0);
                mmio_write(REG_RDT, NUM_RX - 1);
                mmio_write(REG_RCTL, (1<<1)|(1<<2)|(1<<3)|(1<<4)|(1<<15));
                tx_cur = 0;
                rx_cur = 0;
                return 0;
            }
        }
    }
    return -1;
}

int e1000_send(const uint8_t *data, uint32_t len) {
    if (!e1000_mmio || len > BUF_SIZE) return -1;
    uint32_t idx = tx_cur;
    tx_desc_t *d = &tx_ring[idx];
    uint32_t timeout = 100000;
    while (!(d->status & 1) && timeout) timeout--;
    if (!timeout) return -1;
    for (uint32_t i = 0; i < len; i++) tx_bufs[idx][i] = data[i];
    d->length = (uint16_t)len;
    d->cmd = (1<<0)|(1<<1)|(1<<3);
    d->status = 0;
    tx_cur = (idx + 1) % NUM_TX;
    mmio_write(REG_TDT, tx_cur);
    return 0;
}

int e1000_recv(uint8_t *buf, uint32_t max_len) {
    if (!e1000_mmio) return 0;
    uint32_t idx = rx_cur;
    rx_desc_t *d = &rx_ring[idx];
    if (!(d->status & 1)) return 0;
    uint32_t len = d->length;
    if (len == 0 || len > max_len) {
        d->status = 0;
        mmio_write(REG_RDT, idx);
        rx_cur = (idx + 1) % NUM_RX;
        return 0;
    }
    for (uint32_t i = 0; i < len; i++) buf[i] = rx_bufs[idx][i];
    d->status = 0;
    mmio_write(REG_RDT, idx);
    rx_cur = (idx + 1) % NUM_RX;
    return (int)len;
}
int e1000_link_up(void) {
    if (!e1000_mmio) return 0;
    uint32_t status = mmio_read(0x0008);
    return (status & (1 << 1)) ? 1 : 0;
}

void e1000_get_mac(uint8_t out[6]) {
    for (int i = 0; i < 6; i++) out[i] = e1000_mac[i];
}
