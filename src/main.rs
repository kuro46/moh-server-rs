#[macro_use]
extern crate log;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use structopt::StructOpt;
use websocket::sync::Server;
use websocket::{Message, OwnedMessage};

fn main() {
    env_logger::init();

    let server = Server::bind("127.0.0.1:1234").unwrap();
    info!("Waiting connection on 127.0.0.1:1234 ...");

    for connection in server.filter_map(Result::ok) {
        thread::spawn(move || {
            let ws_client = connection.accept().unwrap();
            info!("New connection accepted!");
            let mut tcp_client_reader = TcpStream::connect("127.0.0.1:25565").unwrap();
            let mut tcp_client_writer = tcp_client_reader.try_clone().unwrap();
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

#[derive(StructOpt, Debug)]
#[structopt(name = "Minecraft Over HTTP")]
struct Opt {}
