#ifndef __NVKM_THERM_PRIV_H__
#define __NVKM_THERM_PRIV_H__

#include <stdint.h>

typedef uint8_t  u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

#define ENODEV 19

extern u32 c_nv_rd32(u32 offset);
extern void c_nv_wr32(u32 offset, u32 val);

struct nvkm_device {
    void *gsp;
};

struct nvkm_subdev {
    struct nvkm_device *device;
};

struct nvkm_therm {
    struct nvkm_subdev subdev;
};

enum nvkm_subdev_type {
    NVKM_SUBDEV_THERM = 0
};

struct nvkm_therm_func {
    int (*temp_get)(struct nvkm_therm *therm);
    void (*program_alarms)(struct nvkm_therm *therm);
};

#define nvkm_rd32(dev, addr) c_nv_rd32((addr))
#define nvkm_wr32(dev, addr, val) c_nv_wr32((addr), (val))
#define nvkm_trace(subdev, fmt, ...) ((void)0)

static inline void nvkm_therm_program_alarms_polling(struct nvkm_therm *therm) {
    (void)therm;
}

int gp100_therm_new(struct nvkm_device *device, enum nvkm_subdev_type type, int inst, struct nvkm_therm **ptherm);

static inline int nvkm_therm_new_(const struct nvkm_therm_func *func,
                                  struct nvkm_device *device,
                                  enum nvkm_subdev_type type, int inst,
                                  struct nvkm_therm **ptherm) {
    (void)type; (void)inst; (void)ptherm;
    struct nvkm_therm therm;
    therm.subdev.device = device;
    if (func && func->temp_get) {
        int temp = func->temp_get(&therm);
        if (temp > 0 && temp < 55) {
            /* Регистры PWM вентиляторов из Nouveau (gf119/gm200/gp106):
               запись 0x80000000 выставляет 0% duty cycle и применяет настройку */
            nvkm_wr32(device, 0x00e118, 0x80000000);
            nvkm_wr32(device, 0x00e120, 0x80000000);
            nvkm_wr32(device, 0x000200dc, 0x80000000);
        }
    }
    return 0;
}

int nvidia_c_init(void) {
    struct nvkm_device dev = { 0 };
    struct nvkm_therm *therm = 0;
    return gp100_therm_new(&dev, NVKM_SUBDEV_THERM, 0, &therm);
}

#endif
