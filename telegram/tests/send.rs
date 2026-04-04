use embedded_io_adapters::tokio_1::FromTokio;
use tokio::net::TcpStream;

#[tokio::test]
async fn send_message() {
    let token = std::env::var("TELEGRAM_BOT_TOKEN").unwrap();
    let chat_id = std::env::var("TELEGRAM_CHAT_ID").unwrap();

    let mut conn = FromTokio::new(TcpStream::connect("149.154.167.220:443").await.unwrap());
    let mut rng = rand::thread_rng();
    let mut rx = vec![0u8; 16384];
    let mut tx = vec![0u8; 16384];

    let _ = telegram::send_message(
        &mut conn, &mut rng, &token, &chat_id, "test", &mut rx, &mut tx,
    )
    .await
    .unwrap();
}
