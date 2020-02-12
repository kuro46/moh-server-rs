#[macro_use]
extern crate log;

use serde::Deserialize;
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use websocket::sync::Server;
use websocket::{Message, OwnedMessage};

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
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

    start(config);
}

#[derive(Debug, Deserialize)]
struct Config {
    mc_server_addr: String,
    ws_listen_addr: String,
    proxy_protocol: bool,
}

fn start(config: Config) {
    if config.proxy_protocol {
        info!("Using proxy protocol!");
    }
    let server = Server::bind(&config.ws_listen_addr).unwrap();
    info!("Waiting connection on {} ...", &config.ws_listen_addr);

    let config = Arc::new(config);
    for connection in server.filter_map(Result::ok) {
        let config = Arc::clone(&config);
        thread::spawn(move || {
            let ws_client = connection.accept().unwrap();
            info!(
                "New connection accepted! Connecting to {} ...",
                &config.mc_server_addr
            );
            let mut tcp_client_reader = TcpStream::connect(&config.mc_server_addr).unwrap();
            let mut tcp_client_writer = tcp_client_reader.try_clone().unwrap();
            if config.proxy_protocol {
                let addr = ws_client.peer_addr().unwrap();
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
                    .unwrap();
            }
            let (mut ws_receiver, mut ws_sender) = ws_client.split().unwrap();
            // to server
            thread::spawn(move || {
                for message in ws_receiver.incoming_messages() {
                    debug!("[WS] Received a message");
                    let message = message.unwrap();
                    match message {
                        OwnedMessage::Binary(bin) => {
                            debug!("[TCP] Sending a message");
                            tcp_client_writer.write_all(&bin).unwrap();
                        }
                        OwnedMessage::Close(_data) => {
                            debug!("[WS] Closed");
                            tcp_client_writer
                                .shutdown(std::net::Shutdown::Both)
                                .unwrap();
                            break;
                        }
                        message => warn!("Unsupported message type: {:?}", message),
                    }
                }
            });
            // to client
            loop {
                let mut buf = [0u8; 1024];
                let read = tcp_client_reader.read(&mut buf).unwrap();
                debug!("[TCP] Received a message. Read {} bytes.", read);
                if read == 0 {
                    break;
                }
                debug!("[WS] Sending a message");
                ws_sender
                    .send_message(&Message::binary(&buf[0..read]))
                    .unwrap();
            }
        });
    }
}
