extern crate rand;
extern crate sha1;
pub mod base;
pub mod messages;
pub mod routing;
pub mod server;
use server::{make_server_comms, run_server, ToServerMsg};
use std::io;
use std::thread;

fn main() {
    let (sender, receiver) = make_server_comms();
    thread::spawn(move || {
        if let Err(e) = run_server(receiver, "127.0.0.1:8080") {
            println!("Server died: {}", e);
        }
    });
    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        let _amt = stdin.read_line(&mut line);
        let splits: Vec<&str> = line.split_whitespace().collect();
        let mut sent = false;
        match splits.as_slice() {
            &["store", k, v] => {
                let msg = ToServerMsg::Store(k.into(), v.into());
                if let Err(e) = sender.send(msg) {
                    println!("Error: {}", e);
                } else {
                    sent = true;
                }
            }
            &["get", k] => {
                let msg = ToServerMsg::Get(k.into());
                if let Err(e) = sender.send(msg) {
                    println!("Error: {}", e);
                } else {
                    sent = true;
                }
            }
            _ => println!("Unkown command"),
        }
        line.clear();
        if sent {
            if let Ok(resp) = sender.receive() {
                println!("{:?}", resp);
            }
        }
    }
}
