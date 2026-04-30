mod config;
mod data;
mod filters;
mod logger;
mod net;
mod protocol;

use std::net::SocketAddr;
use std::time::Duration;

use clap::Parser;
use common::{IPT_GROUND, IPT_SELF, IPT_SPAWNS, IPT_TARGET, IPT_WORLD, IPT_ZONE};

use net::ServerConnection;
use protocol::{decode_packet, last_name_from_bytes, name_from_bytes, Packet};

#[derive(Parser)]
#[command(name = "WinShowEQClient", about = "WinShowEQ Rust client")]
struct Cli {
    /// Connect to a WinShowEQ server and print decoded packet summaries (e.g. 127.0.0.1:5555)
    #[arg(long, value_name = "IP:PORT")]
    connect: Option<SocketAddr>,
}

const TICK_REQUEST: i32 =
    IPT_ZONE | IPT_SELF | IPT_TARGET | IPT_SPAWNS | IPT_GROUND | IPT_WORLD;

fn main() {
    let cli = Cli::parse();

    if let Some(addr) = cli.connect {
        run_connect_mode(addr);
    } else {
        println!("WinShowEQ client — GUI mode not yet implemented (C4)");
        println!("Use --connect <ip:port> to connect to a server");
    }
}

fn run_connect_mode(addr: SocketAddr) {
    println!("Connecting to {addr}...");
    let mut conn = match ServerConnection::connect(addr) {
        Ok(c) => {
            println!("Connected.");
            c
        }
        Err(e) => {
            eprintln!("Connection failed: {e}");
            return;
        }
    };

    loop {
        match conn.tick(TICK_REQUEST) {
            Ok(records) => {
                for rec in records {
                    print_packet(decode_packet(rec));
                }
            }
            Err(e) => {
                eprintln!("Tick error: {e}");
                break;
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    conn.disconnect();
}

fn print_packet(packet: Packet) {
    match packet {
        Packet::Zone { name } => println!("[ZONE]    {name}"),
        Packet::Self_(rec) => {
            let (name, x, y, z) = (name_from_bytes(&rec.name), { rec.x }, { rec.y }, { rec.z });
            println!("[SELF]    {name} ({x:.1}, {y:.1}, {z:.1})");
        }
        Packet::Target(rec) => {
            let (name, id) = (name_from_bytes(&rec.name), { rec.id });
            println!("[TARGET]  {name} id={id}");
        }
        Packet::Spawn(rec) => {
            let name = name_from_bytes(&rec.name);
            let last = last_name_from_bytes(&rec.last_name);
            let (id, lvl, x, y, z) =
                ({ rec.id }, rec.level, { rec.x }, { rec.y }, { rec.z });
            let display = if last.is_empty() {
                name
            } else {
                format!("{name} {last}")
            };
            println!("[SPAWN]   {display} id={id} lvl={lvl} ({x:.1}, {y:.1}, {z:.1})");
        }
        Packet::Ground(rec) => {
            let (name, x, y, z) = (name_from_bytes(&rec.name), { rec.x }, { rec.y }, { rec.z });
            println!("[GROUND]  {name} ({x:.1}, {y:.1}, {z:.1})");
        }
        Packet::World(t) => println!("[WORLD]   {}", t.display()),
        Packet::Process { pid } => println!("[PROCESS] pid={pid}"),
        Packet::Unknown { flags } => println!("[UNKNOWN] flags=0x{flags:02X}"),
    }
}