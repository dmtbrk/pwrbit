#![no_std]
#![no_main]

mod dns;
mod uart_log;

use ch9120::{Ch9120, Mode};
use core::sync::atomic::Ordering;
use embassy_executor::Spawner;
use embassy_rp::{
    bind_interrupts,
    clocks::{RoscRng, dormant_sleep},
    dma, gpio,
    peripherals::PIO0,
    pio::Pio,
    pio_programs::ws2812::{Grb, PioWs2812, PioWs2812Program},
    uart,
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    pubsub::{PubSubChannel, Subscriber},
};
use embassy_time::{Delay, Duration, Timer, with_timeout};
use embedded_io_async::Read;
use portable_atomic::AtomicU8;
use smart_leds::RGB8;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

static TASKS_RUNNING: AtomicU8 = AtomicU8::new(0);

fn task_add(n: u8) {
    TASKS_RUNNING.add(n, Ordering::SeqCst);
}

fn task_done() {
    TASKS_RUNNING.sub(1, Ordering::SeqCst);
}

const RED: RGB8 = RGB8::new(0, 1, 0);
const GREEN: RGB8 = RGB8::new(1, 0, 0);
const BLUE: RGB8 = RGB8::new(0, 0, 1);

const DNS_IP: [u8; 4] = [192, 168, 50, 1];

const TELEGRAM_TOKEN: &str = env!("TELEGRAM_BOT_TOKEN");
const TELEGRAM_CHAT_ID: &str = env!("TELEGRAM_CHAT_ID");

static POWER_STATE_WATCH: PubSubChannel<CriticalSectionRawMutex, bool, 4, 2, 1> =
    PubSubChannel::new();

type PowerStateSubscriber = Subscriber<'static, CriticalSectionRawMutex, bool, 4, 2, 1>;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<embassy_rp::peripherals::PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<embassy_rp::peripherals::DMA_CH0>;
    UART1_IRQ => uart::BufferedInterruptHandler<embassy_rp::peripherals::UART1>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    uart_log::init(uart::UartTx::new_blocking(
        p.UART0,
        p.PIN_0,
        Default::default(),
    ));
    log::info!("main: booted");

    static TX_BUF: StaticCell<[u8; 8192]> = StaticCell::new();
    static RX_BUF: StaticCell<[u8; 8192]> = StaticCell::new();

    let mut uart_cfg = uart::Config::default();
    uart_cfg.baudrate = 9600;

    let uart = uart::BufferedUart::new(
        p.UART1,
        p.PIN_20,
        p.PIN_21,
        Irqs,
        TX_BUF.init([0; 8192]),
        RX_BUF.init([0; 8192]),
        uart_cfg,
    );

    let cfg_pin = gpio::Output::new(p.PIN_18, gpio::Level::High);
    let rst_pin = gpio::Output::new(p.PIN_19, gpio::Level::High);
    let mut tcpcs_pin = gpio::Input::new(p.PIN_17, gpio::Pull::Up);

    let mut eth = ch9120::Ch9120::new(uart, cfg_pin, rst_pin, Delay);

    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, Irqs);
    let program = PioWs2812Program::new(&mut common);
    let mut led: PioWs2812<'_, _, 0, 1, _> =
        PioWs2812::new(&mut common, sm0, p.DMA_CH0, Irqs, p.PIN_25, &program);

    match with_timeout(Duration::from_secs(10), async {
        let mut cfg = eth.config().await?;
        cfg.set_dhcp(true).await?;
        cfg.set_mode(Mode::UdpClient).await?;
        cfg.set_target_ip(DNS_IP).await?;
        cfg.set_target_port(53).await?;
        cfg.set_baud_rate(9600).await?;
        cfg.save().await?;
        cfg.exec_and_reset().await?;

        Ok::<_, ch9120::Error>(())
    })
    .await
    {
        Ok(Ok(())) => {
            log::debug!("init: telegram: configuring dns");
            led.write(&[BLUE]).await;
        }
        _ => {
            log::error!("init: telegram: configuring dns: timeout");
            blink(&mut led, RED).await;
        }
    }
    unbreak_eth(&mut eth).await;

    let telegram_ip = match with_timeout(Duration::from_secs(10), async {
        loop {
            log::debug!("init: telegram: resolving ip");
            match with_timeout(
                Duration::from_secs(2),
                dns::resolve_ipv4(&mut eth, "api.telegram.org"),
            )
            .await
            {
                Ok(Ok(ip)) => return ip,
                Ok(Err(e)) => {
                    log::debug!("init: telegram: resolving ip: {:?}", e);
                    Timer::after_secs(1).await;
                }
                Err(_) => {
                    log::debug!("init: telegram: resolving ip: timeout");
                    Timer::after_secs(1).await;
                }
            }
        }
    })
    .await
    {
        Ok(ip) => {
            log::debug!(
                "init: telegram: resolved ip {}.{}.{}.{}",
                ip[0],
                ip[1],
                ip[2],
                ip[3]
            );
            ip
        }
        Err(_) => {
            log::error!("init: telegram: resolving ip: timeout");
            blink(&mut led, RED).await;
        }
    };

    log::debug!(
        "init: telegram: configuring tcp tcpcs_pin={}",
        tcpcs_pin.is_high()
    );
    match with_timeout(Duration::from_secs(10), async {
        let mut cfg = eth.config().await?;
        cfg.set_dhcp(true).await?;
        cfg.set_mode(Mode::TcpClient).await?;
        cfg.set_target_ip(telegram_ip).await?;
        cfg.set_target_port(443).await?;
        cfg.set_baud_rate(9600).await?;
        cfg.save().await?;
        cfg.exec_and_reset().await?;

        Ok::<_, ch9120::Error>(())
    })
    .await
    {
        Ok(Ok(())) => {
            log::debug!("init: telegram: configured tcp");
            led.write(&[BLUE]).await;
        }
        _ => {
            log::error!("init: telegram: configuring tcp: timeout");
            blink(&mut led, RED).await;
        }
    }
    unbreak_eth(&mut eth).await;

    log::debug!(
        "init: telegram: connecting tcpcs_pin={}",
        tcpcs_pin.is_high()
    );
    if with_timeout(Duration::from_secs(60), tcpcs_pin.wait_for_low())
        .await
        .is_err()
    {
        log::error!("init: telegram: connecting: timeout");
        blink(&mut led, RED).await;
    }
    log::debug!(
        "init: telegram: connected tcpcs_pin={}",
        tcpcs_pin.is_high()
    );

    let pub_ = POWER_STATE_WATCH.publisher().unwrap();

    spawner.spawn(led_task(POWER_STATE_WATCH.subscriber().unwrap(), led).unwrap());
    spawner.spawn(telegram_task(POWER_STATE_WATCH.subscriber().unwrap(), eth, tcpcs_pin).unwrap());

    let mut pwr_pin = gpio::Input::new(p.PIN_2, gpio::Pull::Down);

    let mut last_power_state: Option<bool> = None;

    loop {
        let power_state = pwr_pin.is_high();

        if Some(power_state) == last_power_state {
            let n = TASKS_RUNNING.load(Ordering::SeqCst);
            if n > 0 {
                Timer::after_millis(100).await;
                continue;
            }

            log::debug!("main: sleeping zZz");
            log::logger().flush();
            Timer::after_millis(10).await;

            let _wake = pwr_pin.dormant_wake(gpio::DormantWakeConfig {
                edge_high: true,
                edge_low: true,
                level_high: false,
                level_low: false,
            });
            dormant_sleep();

            Timer::after_millis(100).await;
            log::debug!("main: woke up");
            continue;
        }

        last_power_state = Some(power_state);

        log::info!("main: notifying power={}", power_state);

        pub_.publish(power_state).await;
        task_add(2);
    }
}

#[embassy_executor::task]
async fn led_task(mut rcv: PowerStateSubscriber, mut led: PioWs2812<'static, PIO0, 0, 1, Grb>) {
    loop {
        let power_on = rcv.next_message_pure().await;

        log::debug!("led: got power {}", power_on);

        let color = if power_on { GREEN } else { RED };

        led.write(&[color]).await;

        log::debug!(
            "led: updated to {}",
            if color == GREEN { "green" } else { "red" }
        );

        task_done();
    }
}

#[embassy_executor::task]
async fn telegram_task(
    mut rcv: PowerStateSubscriber,
    mut eth: Ch9120<uart::BufferedUart, gpio::Output<'static>, gpio::Output<'static>, Delay>,
    mut tcpcs_pin: gpio::Input<'static>,
) {
    let mut rng = RoscRng;
    let mut rx = [0u8; 8192];
    let mut tx = [0u8; 8192];

    loop {
        let power_on = rcv.next_message_pure().await;

        log::debug!("telegram: got power {}", power_on);

        let msg = if power_on { "power ON" } else { "power OFF" };

        if with_timeout(Duration::from_secs(60), tcpcs_pin.wait_for_low())
            .await
            .is_err()
        {
            log::error!("telegram: timed out waiting for tcp connection");
            task_done();
            continue;
        }

        log::debug!("telegram: sending \"{}\"", msg);
        match with_timeout(
            Duration::from_secs(30),
            telegram::send_message(
                &mut eth,
                &mut rng,
                TELEGRAM_TOKEN,
                TELEGRAM_CHAT_ID,
                msg,
                &mut rx,
                &mut tx,
            ),
        )
        .await
        {
            Err(_) => {
                log::error!("telegram: sending: timeout");
                task_done();
                continue;
            }
            Ok(Err(e)) => {
                log::error!("telegram: sending: {:?}", e);
                task_done();
                continue;
            }
            _ => {}
        }

        log::debug!("teleram: sent \"{}\"", msg);

        task_done();
    }
}

async fn unbreak_eth(
    eth: &mut Ch9120<uart::BufferedUart, gpio::Output<'static>, gpio::Output<'static>, Delay>,
) {
    let mut buf = [0u8; 32];
    match with_timeout(Duration::from_secs(1), eth.read(&mut buf)).await {
        Ok(Ok(n)) => {
            log::debug!("ubreak_eth: read: {:?}", &buf[..n])
        }
        Ok(Err(e)) => log::debug!("unbreak_eth: {:?}", e),
        Err(_) => log::debug!("unbreak_eth: timeout"),
    }
}

async fn blink(led: &mut PioWs2812<'_, PIO0, 0, 1, Grb>, color: RGB8) -> ! {
    loop {
        led.write(&[color]).await;
        Timer::after_millis(500).await;
        led.write(&[RGB8::new(0, 0, 0)]).await;
        Timer::after_millis(500).await;
    }
}
