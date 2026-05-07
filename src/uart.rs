use std::collections::VecDeque;

const UART_DR: u64 = 0x00;
const UART_FR: u64 = 0x18;
const UART_IMSC: u64 = 0x38;
const UART_RIS: u64 = 0x3c;
const UART_MIS: u64 = 0x40;
const UART_ICR: u64 = 0x44;

const UART_FR_RXFE: u32 = 1 << 4;
const UART_FR_TXFE: u32 = 1 << 7;
const UART_INT_RX: u32 = 1 << 4;
const UART_INT_RT: u32 = 1 << 6;

pub struct Uart {
    imsc: u32,
    q: VecDeque<u8>,
}

impl Uart {
    pub fn new() -> Uart {
        Uart {
            imsc: 0,
            q: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, value: u8) -> bool {
        self.q.push_back(value);
        self.q.len() == 1
    }

    fn ris(&self) -> u32 {
        UART_INT_RX * !self.q.is_empty() as u32
    }

    fn mis(&self) -> u32 {
        self.ris() & self.imsc
    }

    pub fn is_asserted(&self) -> bool {
        self.mis() != 0
    }

    pub fn read(&mut self, offset: u64) -> u32 {
        match offset {
            UART_FR => (self.q.is_empty() as u32 * UART_FR_RXFE) | UART_FR_TXFE,

            UART_DR => self.q.pop_front().unwrap_or(0) as u32,

            UART_RIS => self.ris(),

            UART_MIS => self.mis(),

            _ => {
                // println!("Unexpected PL011 register read: offset={}", offset);
                // For bring-up, unknown PL011 registers read as 0.
                0
            }
        }
    }

    pub fn write<F: Fn(u32)>(&mut self, offset: u64, value: u32, on_data: F) {
        match offset {
            UART_DR => {
                on_data(value);
            }

            UART_IMSC => self.imsc = value,

            _ => {
                // Ignore config writes for now:
                // baud divisor, line control, control register, interrupt clear, etc.
            }
        }
    }
}
