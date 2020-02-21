#[macro_use]
extern crate log;

use futures_util::sink::SinkExt;
use futures_util::stream::StreamExt;
use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    openssl_probe::init_ssl_cert_env_vars();

    info!("moh-server v{}", VERSION);

    let config_path = Path::new("./config.toml");
    if !config_path.exists() {
        warn!("Configuration file not exists! Creating...");
        let mut file = File::create(&config_path).unwrap();
        file.write_all(include_bytes!("../resources/config.toml"))
            .unwrap();
        warn!("Configuration file (config.toml) has been created! Please check!");
        return;
    }
    let mut config_file = File::open(&config_path).unwrap();
    let config: Config = {
        let mut buf = String::new();
        config_file.read_to_string(&mut buf).unwrap();
        toml::from_str(&buf).unwrap()
    };

    start(config).await;
}

#[derive(Debug, Deserialize)]
struct Config {
    mc_server_addr: String,
    ws_listen_addr: String,
    proxy_protocol: bool,
}

async fn start(config: Config) {
    if config.proxy_protocol {
        info!("Using proxy protocol!");
    }
    let mut server = TcpListener::bind(&config.ws_listen_addr).await.unwrap();
    info!("Waiting connection on {} ...", &config.ws_listen_addr);

    let config = Arc::new(config);
    while let Ok((connection, _)) = server.accept().await {
        let config = Arc::clone(&config);
        tokio::spawn(async move {
            connection.set_nodelay(true).unwrap();
            let ws_client = accept_async(connection).await.unwrap();
            info!(
                "New connection accepted! Connecting to {} ...",
                &config.mc_server_addr
            );
            let tcp_client = TcpStream::connect(&config.mc_server_addr).await.unwrap();
            tcp_client.set_nodelay(true).unwrap();
            let (mut tcp_client_reader, mut tcp_client_writer) = tokio::io::split(tcp_client);
            if config.proxy_protocol {
                let addr = ws_client.get_ref().peer_addr().unwrap();
                let is_v4 = addr.is_ipv4();
                let version = if is_v4 { "4" } else { "6" };
                let local_ip = if is_v4 { "127.0.0.1" } else { "::1" };
                let remote_ip = &format!("{}", addr.ip());
                tcp_client_writer
                    .write_all(
                        format!(
                            "PROXY TCP{} {} {} 25565 25565\r\n",
                            version, remote_ip, local_ip
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            let (mut ws_sender, mut ws_receiver) = ws_client.split();
            // to server
            tokio::spawn(async move {
                while let Some(message) = ws_receiver.next().await {
                    debug!("[WS] Received a message");
                    let message = message.unwrap();
                    match message {
                        Message::Binary(bin) => {
                            debug!("[TCP] Sending a message");
                            tcp_client_writer.write_all(&bin).await.unwrap();
                        }
                        Message::Close(_data) => {
                            debug!("[WS] Connection closed by client");
                            //                            tcp_client_to_shutdown
                            //                                .shutdown(std::net::Shutdown::Both)
                            //                                .unwrap();
                            break;
                        }
                        message => warn!("Unsupported message type: {:?}", message),
                    }
                }
            });
            // to client
            loop {
                let mut buf = [0u8; 1024];
                let read = tcp_client_reader.read(&mut buf).await.unwrap();
                debug!("[TCP] Received a message. Read {} bytes.", read);
                if read == 0 {
                    info!("[TCP] Connection is closed by server.");
                    break;
                }
                debug!("[WS] Sending a message");
                ws_sender
                    .send(Message::binary(&buf[0..read]))
                    .await
                    .unwrap();
            }
        });
    }
}
