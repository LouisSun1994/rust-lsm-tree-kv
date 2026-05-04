//! 簡單的命令列 demo：
//!   cargo run --release -- <data_dir> put <key> <value>
//!   cargo run --release -- <data_dir> get <key>
//!   cargo run --release -- <data_dir> delete <key>
//!   cargo run --release -- <data_dir> flush
//!   cargo run --release -- <data_dir> stats
//!   cargo run --release -- <data_dir> bench <n>
//!
//! data_dir 是儲存目錄，不存在會自動建立。

use lsm_kv::LsmEngine;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!(
            "用法：\n  {0} <dir> put <key> <value>\n  {0} <dir> get <key>\n  {0} <dir> delete <key>\n  {0} <dir> flush\n  {0} <dir> stats\n  {0} <dir> bench <n>",
            args[0]
        );
        std::process::exit(1);
    }
    let dir = &args[1];
    let cmd = args[2].as_str();

    let mut db = LsmEngine::open(dir).expect("open db");

    match cmd {
        "put" => {
            let key = args.get(3).expect("missing key");
            let value = args.get(4).expect("missing value");
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
            println!("OK");
        }
        "get" => {
            let key = args.get(3).expect("missing key");
            match db.get(key.as_bytes()).unwrap() {
                Some(v) => println!("{}", String::from_utf8_lossy(&v)),
                None => {
                    println!("(not found)");
                    std::process::exit(2);
                }
            }
        }
        "delete" => {
            let key = args.get(3).expect("missing key");
            db.delete(key.as_bytes()).unwrap();
            println!("OK");
        }
        "flush" => {
            db.flush().unwrap();
            println!("flushed; sstables = {}", db.num_sstables());
        }
        "stats" => {
            println!("sstables: {}", db.num_sstables());
        }
        "bench" => {
            let n: usize = args.get(3).expect("missing n").parse().expect("n must be int");
            let start = Instant::now();
            for i in 0..n {
                let k = format!("key{:08}", i);
                let v = format!("value-{}", i);
                db.put(k.as_bytes(), v.as_bytes()).unwrap();
            }
            let write_elapsed = start.elapsed();
            println!(
                "wrote {} entries in {:?}  ({:.0} ops/sec)",
                n,
                write_elapsed,
                n as f64 / write_elapsed.as_secs_f64()
            );

            let start = Instant::now();
            let mut hit = 0usize;
            for i in 0..n {
                let k = format!("key{:08}", i);
                if db.get(k.as_bytes()).unwrap().is_some() {
                    hit += 1;
                }
            }
            let read_elapsed = start.elapsed();
            println!(
                "read  {} entries in {:?}  ({:.0} ops/sec, hits={})",
                n,
                read_elapsed,
                n as f64 / read_elapsed.as_secs_f64(),
                hit
            );
        }
        other => {
            eprintln!("未知命令：{}", other);
            std::process::exit(1);
        }
    }
}
