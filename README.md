# rust-lsm-tree-kv

從零實作的 LSM-Tree Key-Value 儲存引擎，**用來學 Rust + 學系統程式設計**的啟蒙專案。

## 這是什麼

一個能跑的迷你資料庫核心。約 700 行 Rust，包含：

- `MemTable` —— 記憶體中的有序寫入緩衝（用 `BTreeMap` 實作）
- `WAL` —— Write-Ahead Log，斷電後能還原資料
- `SSTable` —— 磁碟上的有序、不可變資料檔
- `LsmEngine` —— 把上面三個組起來的對外 API
- 16 個單元測試，全部通過
- CLI 工具，可以直接 `put` / `get` / `delete` / `bench`

## 快速開始

```bash
# 編譯
cargo build

# 跑全部測試
cargo test

# 用 CLI 玩玩看（資料會存到 ./data/）
cargo run --release -- ./data put name Louis
cargo run --release -- ./data put role engineer
cargo run --release -- ./data get name
cargo run --release -- ./data delete name
cargo run --release -- ./data flush
cargo run --release -- ./data bench 10000
```

## 學習文檔（按順序讀）

| 順序 | 文件 | 內容 |
|---|---|---|
| 1 | [docs/01-lsm-tree-concepts.md](docs/01-lsm-tree-concepts.md) | **LSM-Tree 核心概念**（從原理講起，為什麼這樣設計）|
| 2 | [docs/02-rust-quickstart.md](docs/02-rust-quickstart.md) | Rust 入門：你會在這個專案看到的語法與概念 |
| 3 | [docs/03-code-walkthrough.md](docs/03-code-walkthrough.md) | 逐檔程式碼導讀，配合中文註解 |
| 4 | [docs/04-next-steps.md](docs/04-next-steps.md) | 下一步該挑戰什麼（Compaction、Bloom Filter、並發…）|

## 專案結構

```
rust-lsm-tree-kv/
├── Cargo.toml          # Rust 的 package.json
├── Cargo.lock          # 鎖定的依賴版本
├── src/
│   ├── lib.rs          # crate 的入口（library 部分）
│   ├── main.rs         # CLI 程式（binary 部分）
│   ├── memtable.rs     # MemTable
│   ├── wal.rs          # Write-Ahead Log
│   ├── sstable.rs      # SSTable 讀寫
│   └── engine.rs       # LsmEngine（對外 API）
├── docs/               # 學習文檔（中文）
└── target/             # cargo 編譯產物（.gitignore）
```

## 這份實作刻意省略的東西

為了讓你能看完整份程式碼而不是被細節淹沒，下列「工業級」功能沒做：

- **Compaction**：SSTable 不會自動合併，跑久了讀取會變慢。
- **Bloom Filter**：每次讀 miss 都會掃過所有 SSTable。
- **Block-based SSTable**：SSTable 的 index 是整段，沒分塊。
- **並發**：所有 API 是 `&mut self`，單執行緒。
- **Manifest 檔**：靠目錄掃描推斷狀態，沒有正式的 metadata 檔。
- **壓縮**：value 直接寫，沒有 zstd / snappy。

[docs/04-next-steps.md](docs/04-next-steps.md) 詳細說明每一項該怎麼補。

## 性能基準（M 系列 Mac，release build）

```
wrote 50000 entries in 198s  (252 ops/sec)
read  50000 entries in 76ms  (658303 ops/sec)
```

寫入慢的原因是**每筆 put 都 fsync 一次 WAL**。這是學習目的下的刻意選擇 ——
真實系統會用 group commit（積到 N 筆才 fsync 一次）把寫入吞吐拉到數萬 ops/sec。
這個權衡在 [docs/01-lsm-tree-concepts.md](docs/01-lsm-tree-concepts.md) 裡有細談。
