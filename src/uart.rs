const UART_DR: u64 = 0x00;
const UART_FR: u64 = 0x18;

const UART_FR_RXFE: u64 = 1 << 4;
const UART_FR_TXFE: u64 = 1 << 7;

pub fn pl011_read(offset: u64) -> u64 {
    match offset {
        UART_FR => {
            // No input pending, TX is immediately empty, not busy.
            UART_FR_RXFE | UART_FR_TXFE
        }

        UART_DR => {
            // No input support yet.
            0
        }

        _ => {
            // For bring-up, unknown PL011 registers read as 0.
            0
        }
    }
}

pub fn pl011_write(offset: u64, value: u64) {
    match offset {
        UART_DR => {
            let byte = (value & 0xff) as u8;
            print!("{}", char::from(byte));
        }

        _ => {
            // Ignore config writes for now:
            // baud divisor, line control, control register, interrupt clear, etc.
        }
    }
}
