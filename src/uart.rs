use crate::{irq::IrqLine, stdio};
use std::collections::VecDeque;

const UART_DR: u64 = 0x00;
const UART_FR: u64 = 0x18;
const UART_IMSC: u64 = 0x38;
const UART_RIS: u64 = 0x3c;
const UART_MIS: u64 = 0x40;

const UART_FR_RXFE: u32 = 1 << 4;
const UART_FR_TXFE: u32 = 1 << 7;
const UART_INT_RX: u32 = 1 << 4;

pub struct Uart {
    imsc: u32,
    q: VecDeque<u8>,
    irq: IrqLine,
    serial: stdio::Sink,
}

impl Uart {
    pub fn new(irq: IrqLine, serial: stdio::Sink) -> Self {
        Self {
            imsc: 0,
            q: VecDeque::new(),
            irq,
            serial,
        }
    }

    pub fn enqueue(&mut self, value: u8) {
        self.q.push_back(value);
        self.sync_irq();
    }

    fn ris(&self) -> u32 {
        UART_INT_RX * !self.q.is_empty() as u32
    }

    fn mis(&self) -> u32 {
        self.ris() & self.imsc
    }

    fn sync_irq(&mut self) {
        self.irq.set(self.mis() != 0);
    }

    pub fn read(&mut self, offset: u64) -> u32 {
        let value = match offset {
            UART_FR => (self.q.is_empty() as u32 * UART_FR_RXFE) | UART_FR_TXFE,
            UART_DR => self.q.pop_front().unwrap_or(0) as u32,
            UART_RIS => self.ris(),
            UART_MIS => self.mis(),
            _ => 0,
        };

        self.sync_irq();
        value
    }

    pub fn write(&mut self, offset: u64, value: u32) {
        match offset {
            UART_DR => self.serial.write(value as u8),
            UART_IMSC => self.imsc = value,
            _ => {}
        }

        self.sync_irq();
    }
}
