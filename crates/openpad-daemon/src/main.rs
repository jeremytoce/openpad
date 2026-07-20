use openpad_daemon::input::{spawn_listener, PhysKey};
use std::sync::mpsc::channel;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("listen") => listen(),
        _ => usage(),
    }
}

fn usage() {
    println!("usage: openpad <command>");
    println!();
    println!("commands:");
    println!("  listen   debug: print PhysKey events from the pad as they arrive");
}

fn listen() {
    let (tx, rx) = channel::<PhysKey>();
    spawn_listener(tx);
    println!("listening for pad input (Ctrl+C to quit)...");
    for event in rx {
        println!("{event:?}");
    }
}
