use vm_fdt::{FdtWriter, FdtWriterResult};

pub const RAM_START: u64 = 0x40000000;
pub const KERNEL_TEXT_OFFSET: u64 = 0x0;
pub const IMAGE_START: u64 = RAM_START + KERNEL_TEXT_OFFSET;
pub const INITRD_START: u64 = 0x48000000;
pub const DTB_START: u64 = 0x4F000000;
pub const GICD_START: u64 = 0x08000000;
pub const GICR_START: u64 = 0x080A0000;
pub const UART_START: u64 = 0x09000000;
pub const UART_SIZE: u64 = 0x1000;
pub const UART_SPI_OFFSET: u32 = 1;

pub const VIRTBLK_START: u64 = 0x0a000000;
pub const VIRTBLK_SIZE: u64 = 0x1000;
pub const VIRTBLK_SPI_OFFSET: u32 = 32;

pub const VIRTNET_START: u64 = 0x0a001000;
pub const VIRTNET_SIZE: u64 = 0x1000;
pub const VIRTNET_SPI_OFFSET: u32 = 33;

pub const VIRTGPU_START: u64 = 0x0a002000;
pub const VIRTGPU_SIZE: u64 = 0x1000;
pub const VIRTGPU_SPI_OFFSET: u32 = 34;

pub const VIRTINPUT_KEYBOARD_START: u64 = 0x0a003000;
pub const VIRTINPUT_KEYBOARD_SIZE: u64 = 0x1000;
pub const VIRTINPUT_KEYBOARD_SPI_OFFSET: u32 = 35;

pub const VIRTINPUT_TABLET_START: u64 = 0x0a004000;
pub const VIRTINPUT_TABLET_SIZE: u64 = 0x1000;
pub const VIRTINPUT_TABLET_SPI_OFFSET: u32 = 36;

pub const VIRTSND_START: u64 = 0x0a005000;
pub const VIRTSND_SIZE: u64 = 0x1000;
pub const VIRTSND_SPI_OFFSET: u32 = 37;

pub const VIRTCONSOLE_START: u64 = 0x0a006000;
pub const VIRTCONSOLE_SIZE: u64 = 0x1000;
pub const VIRTCONSOLE_SPI_OFFSET: u32 = 38;

pub const VIRTINPUT_MOUSE_START: u64 = 0x0a007000;
pub const VIRTINPUT_MOUSE_SIZE: u64 = 0x1000;
pub const VIRTINPUT_MOUSE_SPI_OFFSET: u32 = 39;

const GIC_PHANDLE: u32 = 1;
const CLOCK_PHANDLE: u32 = 2;

pub fn build_fdt(
    memory_size: u64,
    vcpus: usize,
    kernel_args: &str,
    initrd: Option<(u64, u64)>,
    gicd_size: u64,
    gicr_size: u64,
) -> FdtWriterResult<Vec<u8>> {
    let mut fdt = FdtWriter::new()?;
    fdt.set_boot_cpuid_phys(0);

    let root = fdt.begin_node("")?;
    fdt.property_string("compatible", "varmint,virt")?;
    fdt.property_u32("#address-cells", 2)?;
    fdt.property_u32("#size-cells", 2)?;
    fdt.property_u32("interrupt-parent", GIC_PHANDLE)?;

    let aliases = fdt.begin_node("aliases")?;
    fdt.property_string("serial0", "/serial@9000000")?;
    fdt.end_node(aliases)?;

    let chosen = fdt.begin_node("chosen")?;
    fdt.property_string("bootargs", kernel_args)?;
    fdt.property_string("stdout-path", "serial0:115200n8")?;
    if let Some((start, end)) = initrd {
        fdt.property_u64("linux,initrd-start", start)?;
        fdt.property_u64("linux,initrd-end", end)?;
    }
    fdt.end_node(chosen)?;

    let psci = fdt.begin_node("psci")?;
    fdt.property_string("compatible", "arm,psci-0.2")?;
    fdt.property_string("method", "hvc")?;
    fdt.end_node(psci)?;

    let cpus = fdt.begin_node("cpus")?;
    fdt.property_u32("#address-cells", 1)?;
    fdt.property_u32("#size-cells", 0)?;
    for cpu_id in 0..vcpus {
        let cpu = fdt.begin_node(&format!("cpu@{cpu_id:x}"))?;
        fdt.property_string("device_type", "cpu")?;
        fdt.property_string("compatible", "arm,armv8")?;
        fdt.property_u32("reg", cpu_id as u32)?;
        fdt.property_string("enable-method", "psci")?;
        fdt.end_node(cpu)?;
    }
    fdt.end_node(cpus)?;

    let gic = fdt.begin_node(&format!("interrupt-controller@{GICD_START:x}"))?;
    fdt.property_string("compatible", "arm,gic-v3")?;
    fdt.property_null("interrupt-controller")?;
    fdt.property_u32("#interrupt-cells", 3)?;
    fdt.property_array_u64("reg", &[GICD_START, gicd_size, GICR_START, gicr_size])?;
    fdt.property_phandle(GIC_PHANDLE)?;
    fdt.end_node(gic)?;

    let timer = fdt.begin_node("timer")?;
    fdt.property_string("compatible", "arm,armv8-timer")?;
    fdt.property_array_u32("interrupts", &[1, 13, 0xf08, 1, 14, 0xf08, 1, 11, 0xf08, 1, 10, 0xf08])?;
    fdt.end_node(timer)?;

    let memory = fdt.begin_node(&format!("memory@{RAM_START:x}"))?;
    fdt.property_string("device_type", "memory")?;
    fdt.property_array_u64("reg", &[RAM_START, memory_size])?;
    fdt.end_node(memory)?;

    let clock = fdt.begin_node("clk24m")?;
    fdt.property_string("compatible", "fixed-clock")?;
    fdt.property_u32("#clock-cells", 0)?;
    fdt.property_u32("clock-frequency", 24_000_000)?;
    fdt.property_phandle(CLOCK_PHANDLE)?;
    fdt.end_node(clock)?;

    let uart = fdt.begin_node(&format!("serial@{UART_START:x}"))?;
    fdt.property_string_list("compatible", vec!["arm,pl011".to_owned(), "arm,primecell".to_owned()])?;
    fdt.property_u32("arm,primecell-periphid", 0x0034_1011)?;
    fdt.property_array_u64("reg", &[UART_START, UART_SIZE])?;
    fdt.property_array_u32("interrupts", &[0, UART_SPI_OFFSET, 4])?;
    fdt.property_array_u32("clocks", &[CLOCK_PHANDLE, CLOCK_PHANDLE])?;
    fdt.property_string_list("clock-names", vec!["uartclk".to_owned(), "apb_pclk".to_owned()])?;
    fdt.property_string("status", "okay")?;
    fdt.end_node(uart)?;

    add_virtio(&mut fdt, "virtio_blk", VIRTBLK_START, VIRTBLK_SIZE, VIRTBLK_SPI_OFFSET)?;
    add_virtio(&mut fdt, "virtio_net", VIRTNET_START, VIRTNET_SIZE, VIRTNET_SPI_OFFSET)?;
    add_virtio(&mut fdt, "virtio_gpu", VIRTGPU_START, VIRTGPU_SIZE, VIRTGPU_SPI_OFFSET)?;
    add_virtio(
        &mut fdt,
        "virtio_input_keyboard",
        VIRTINPUT_KEYBOARD_START,
        VIRTINPUT_KEYBOARD_SIZE,
        VIRTINPUT_KEYBOARD_SPI_OFFSET,
    )?;
    add_virtio(
        &mut fdt,
        "virtio_input_tablet",
        VIRTINPUT_TABLET_START,
        VIRTINPUT_TABLET_SIZE,
        VIRTINPUT_TABLET_SPI_OFFSET,
    )?;
    add_virtio(&mut fdt, "virtio_snd", VIRTSND_START, VIRTSND_SIZE, VIRTSND_SPI_OFFSET)?;
    add_virtio(
        &mut fdt,
        "virtio_console",
        VIRTCONSOLE_START,
        VIRTCONSOLE_SIZE,
        VIRTCONSOLE_SPI_OFFSET,
    )?;
    add_virtio(
        &mut fdt,
        "virtio_input_mouse",
        VIRTINPUT_MOUSE_START,
        VIRTINPUT_MOUSE_SIZE,
        VIRTINPUT_MOUSE_SPI_OFFSET,
    )?;

    fdt.end_node(root)?;
    fdt.finish()
}

fn add_virtio(fdt: &mut FdtWriter, name: &str, base: u64, size: u64, interrupt: u32) -> FdtWriterResult<()> {
    let node = fdt.begin_node(&format!("{name}@{base:x}"))?;
    fdt.property_string("compatible", "virtio,mmio")?;
    fdt.property_array_u64("reg", &[base, size])?;
    fdt.property_array_u32("interrupts", &[0, interrupt, 4])?;
    fdt.end_node(node)
}
