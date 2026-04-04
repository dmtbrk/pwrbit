use core::cell::RefCell;
use core::fmt::Write;

use embassy_rp::uart;
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use embassy_time::Instant;
static LOGGER: UartLogger = UartLogger(Mutex::new(RefCell::new(None)));

pub fn init(tx: uart::UartTx<'static, uart::Blocking>) {
    LOGGER.0.lock(|inner| inner.borrow_mut().replace(tx));
    unsafe { log::set_logger_racy(&LOGGER) }.ok();
    unsafe { log::set_max_level_racy(log::LevelFilter::Info) };
}

pub struct UartLogger(
    Mutex<CriticalSectionRawMutex, RefCell<Option<uart::UartTx<'static, uart::Blocking>>>>,
);

impl log::Log for UartLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let now = Instant::now();
        self.0.lock(|tx| {
            if let Some(tx) = tx.borrow_mut().as_mut() {
                let _ = write!(
                    UartWriter(tx),
                    "{:>6}.{:03} {:<5} {}\r\n",
                    now.as_secs(),
                    now.as_millis() % 1000,
                    record.level(),
                    record.args()
                );
            }
        })
    }

    fn flush(&self) {
        self.0.lock(|tx| {
            if let Some(tx) = tx.borrow_mut().as_mut() {
                let _ = tx.blocking_flush();
            }
        })
    }
}

struct UartWriter<'a>(&'a mut uart::UartTx<'static, uart::Blocking>);

impl core::fmt::Write for UartWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0
            .blocking_write(s.as_bytes())
            .map_err(|_| core::fmt::Error)
    }
}
