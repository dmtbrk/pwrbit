use dns_protocol::{Error as DnsError, Flags, Message, Question, ResourceRecord, ResourceType};
use embedded_io_async::{ErrorType, Read, Write};

#[derive(Debug)]
#[allow(dead_code)]
pub enum Error<E> {
    Transport(E),
    Dns(DnsError),
    BufferTooSmall,
    NoRecord,
}

pub async fn resolve_ipv4<T>(transport: &mut T, hostname: &str) -> Result<[u8; 4], Error<T::Error>>
where
    T: Read + Write + ErrorType,
    T::Error: core::fmt::Debug,
{
    let mut buf = [0u8; 1024];

    let mut questions = [Question::new(hostname, ResourceType::A, 1)];
    let mut answers = [ResourceRecord::default(); 4];

    let msg = Message::new(
        0x1,
        Flags::standard_query(),
        &mut questions,
        &mut [],
        &mut [],
        &mut [],
    );

    if buf.len() < msg.space_needed() {
        return Err(Error::BufferTooSmall);
    }
    let mut len = msg.write(&mut buf).map_err(Error::Dns)?;
    transport
        .write_all(&buf[..len])
        .await
        .map_err(Error::Transport)?;

    len = transport.read(&mut buf).await.map_err(Error::Transport)?;
    let resp = Message::read(&buf[..len], &mut questions, &mut answers, &mut [], &mut [])
        .map_err(Error::Dns)?;

    for answer in resp.answers() {
        if answer.ty() == ResourceType::A && answer.data().len() == 4 {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(answer.data());
            return Ok(ip);
        }
    }

    Err(Error::NoRecord)
}
