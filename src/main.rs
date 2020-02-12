use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use websocket::sync::Server;
use websocket::{Message, OwnedMessage};

fn main() {
    let server = Server::bind("127.0.0.1:1234").unwrap();

    for connection in server.filter_map(Result::ok) {
        // Spawn a new thread for each connection.
        thread::spawn(move || {
            let ws_client = connection.accept().unwrap();
            let mut tcp_client_reader = TcpStream::connect("127.0.0.1:25565").unwrap();
            let mut tcp_client_writer = tcp_client_reader.try_clone().unwrap();
            let (mut ws_receiver, mut ws_sender) = ws_client.split().unwrap();
            // to server
            thread::spawn(move || {
                for message in ws_receiver.incoming_messages() {
                    let message = message.unwrap();
                    println!("Recv: {:?}", message);
                    if let OwnedMessage::Binary(bin) = message {
                        tcp_client_writer.write_all(&bin).unwrap();
                    }
                }
            });
            // to client
            loop {
                let mut buf = [0u8; 1024];
                let read = tcp_client_reader.read(&mut buf).unwrap();
                ws_sender
                    .send_message(&Message::binary(&buf[0..read]))
                    .unwrap();
            }
        });
    }
}
