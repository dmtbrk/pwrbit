#![no_std]

use core::fmt::Write as FmtWrite;
use embedded_io_async::Write;
use embedded_tls::{
    Aes256GcmSha384, CryptoRngCore, TlsConfig, TlsConnection, TlsContext, TlsError,
    UnsecureProvider,
};

#[derive(Debug)]
pub enum Error {
    Tls(TlsError),
    Format(core::fmt::Error),
    Utf8(core::str::Utf8Error),
    Http,
}

impl From<TlsError> for Error {
    fn from(e: TlsError) -> Self {
        Error::Tls(e)
    }
}

impl From<core::fmt::Error> for Error {
    fn from(e: core::fmt::Error) -> Self {
        Error::Format(e)
    }
}

impl From<core::str::Utf8Error> for Error {
    fn from(e: core::str::Utf8Error) -> Self {
        Error::Utf8(e)
    }
}

pub async fn send_message<T, R>(
    transport: &mut T,
    rng: &mut R,
    token: &str,
    chat_id: &str,
    text: &str,
    rx_buf: &mut [u8],
    tx_buf: &mut [u8],
) -> Result<(), Error>
where
    T: embedded_io_async::Read + embedded_io_async::Write,
    R: CryptoRngCore,
{
    let cfg = TlsConfig::new()
        .with_server_name("api.telegram.org")
        .enable_rsa_signatures();
    let mut conn: TlsConnection<'_, _, Aes256GcmSha384> =
        TlsConnection::new(transport, rx_buf, tx_buf);

    conn.open(TlsContext::new(
        &cfg,
        UnsecureProvider::new::<Aes256GcmSha384>(rng),
    ))
    .await?;

    let mut body = heapless::String::<256>::new();
    write!(body, r#"{{"chat_id":"{}", "text":"{}"}}"#, chat_id, text)?;

    let mut header = heapless::String::<512>::new();
    write!(
        header,
        "POST /bot{}/sendMessage HTTP/1.1\r\n\
           Host: api.telegram.org\r\n\
           Content-Type: application/json\r\n\
           Content-Length: {}\r\n\
           Connection: close\r\n\
           \r\n",
        token,
        body.len()
    )?;

    conn.write_all(header.as_bytes()).await?;
    conn.write_all(body.as_bytes()).await?;
    conn.flush().await?;

    let mut buf = [0u8; 256];
    let n = conn.read(&mut buf).await?;
    let resp = core::str::from_utf8(&buf[..n])?;

    if !resp.starts_with("HTTP/1.1 200") {
        return Err(Error::Http);
    }

    Ok(())
}
