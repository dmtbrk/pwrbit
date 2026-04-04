#![no_std]

use embedded_hal::digital::OutputPin;
use embedded_hal_async::delay::DelayNs;
use embedded_io_async::{Read, Write};

const CMD_SAVE: u8 = 0x0D;
const CMD_EXEC_AND_RESET: u8 = 0x0E;
const CMD_MODE: u8 = 0x10;
const CMD_LOCAL_PORT: u8 = 0x14;
const CMD_TARGET_IP: u8 = 0x15;
const CMD_TARGET_PORT: u8 = 0x16;
const CMD_BAUD_RATE: u8 = 0x21;
const CMD_DHCP: u8 = 0x33;

#[repr(u8)]
pub enum Mode {
    TcpServer = 0x0,
    TcpClient = 0x1,
    UdpServer = 0x2,
    UdpClient = 0x3,
}

#[derive(Debug)]
pub enum Error {
    Uart,
    Pin,
    Nak(u8),
}

pub struct Ch9120<Uart, Cfg, Rst, Delay> {
    uart: Uart,
    cfg_pin: Cfg,
    rst_pin: Rst,
    delay: Delay,
}

impl<Uart, Cfg, Rst, Delay> Ch9120<Uart, Cfg, Rst, Delay>
where
    Uart: Read + Write,
    Cfg: OutputPin,
    Rst: OutputPin,
    Delay: DelayNs,
{
    pub fn new(uart: Uart, cfg_pin: Cfg, rst_pin: Rst, delay: Delay) -> Self {
        Self {
            uart,
            cfg_pin,
            rst_pin,
            delay,
        }
    }

    pub async fn config(&mut self) -> Result<Config<'_, Uart, Cfg, Rst, Delay>, Error> {
        self.cfg_pin.set_low().map_err(|_| Error::Pin)?;
        self.delay.delay_ms(50).await;
        Ok(Config { ch9120: self })
    }

    pub async fn reset(&mut self) -> Result<(), Error> {
        self.rst_pin.set_low().map_err(|_| Error::Pin)?;
        self.delay.delay_ms(200).await;
        self.rst_pin.set_high().map_err(|_| Error::Pin)
    }
}

pub struct Config<'a, Uart, Cfg, Rst, Delay>
where
    Cfg: OutputPin,
{
    ch9120: &'a mut Ch9120<Uart, Cfg, Rst, Delay>,
}

impl<Uart, Cfg, Rst, Delay> Config<'_, Uart, Cfg, Rst, Delay>
where
    Uart: Read + Write,
    Cfg: OutputPin,
    Delay: DelayNs,
{
    pub async fn set_mode(&mut self, mode: Mode) -> Result<(), Error> {
        self.send_command(CMD_MODE, &[mode as u8]).await
    }

    pub async fn set_target_ip(&mut self, ip: [u8; 4]) -> Result<(), Error> {
        self.send_command(CMD_TARGET_IP, &ip).await
    }
    pub async fn set_target_port(&mut self, port: u16) -> Result<(), Error> {
        self.send_command(CMD_TARGET_PORT, &port.to_le_bytes())
            .await
    }
    pub async fn set_local_port(&mut self, port: u16) -> Result<(), Error> {
        self.send_command(CMD_LOCAL_PORT, &port.to_le_bytes()).await
    }
    pub async fn set_baud_rate(&mut self, baud: u32) -> Result<(), Error> {
        self.send_command(CMD_BAUD_RATE, &baud.to_le_bytes()).await
    }
    pub async fn set_dhcp(&mut self, enabled: bool) -> Result<(), Error> {
        self.send_command(CMD_DHCP, &[enabled.into()]).await
    }
    pub async fn save(&mut self) -> Result<(), Error> {
        self.send_command(CMD_SAVE, &[]).await
    }
    pub async fn exec_and_reset(&mut self) -> Result<(), Error> {
        self.send_command(CMD_EXEC_AND_RESET, &[]).await
    }

    async fn send_command(&mut self, cmd: u8, data: &[u8]) -> Result<(), Error> {
        self.ch9120
            .uart
            .write_all(&[0x57, 0xAB, cmd])
            .await
            .map_err(|_| Error::Uart)?;
        self.ch9120
            .uart
            .write_all(data)
            .await
            .map_err(|_| Error::Uart)?;
        let mut res: [u8; 1] = [0x0];
        self.ch9120
            .uart
            .read_exact(&mut res)
            .await
            .map_err(|_| Error::Uart)?;
        if res[0] != 0xAA {
            return Err(Error::Nak(res[0]));
        }
        Ok(())
    }
}

impl<Uart, Cfg, Rst, Delay> Drop for Config<'_, Uart, Cfg, Rst, Delay>
where
    Cfg: OutputPin,
{
    fn drop(&mut self) {
        self.ch9120.cfg_pin.set_high().ok();
    }
}

impl<Uart, Cfg, Rst, Delay> embedded_io_async::ErrorType for Ch9120<Uart, Cfg, Rst, Delay>
where
    Uart: embedded_io_async::ErrorType,
{
    type Error = Uart::Error;
}

impl<Uart, Cfg, Rst, Delay> embedded_io_async::Read for Ch9120<Uart, Cfg, Rst, Delay>
where
    Uart: embedded_io_async::Read,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.uart.read(buf).await
    }
}

impl<Uart, Cfg, Rst, Delay> embedded_io_async::Write for Ch9120<Uart, Cfg, Rst, Delay>
where
    Uart: embedded_io_async::Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.uart.write(buf).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.uart.flush().await
    }
}
