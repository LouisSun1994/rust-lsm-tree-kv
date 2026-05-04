# 程式碼導讀

> 配著實際的 .rs 檔一起看。每節都會點出「這段程式碼的 LSM 觀念」+「這段程式碼的 Rust 觀念」。

## 0. 整體結構

```
src/
├── lib.rs        ← crate 根模組，宣告 pub mod & Error 型別
├── main.rs       ← CLI binary
├── memtable.rs   ← MemTable
├── wal.rs        ← Write-Ahead Log
├── sstable.rs    ← SSTable 讀寫
└── engine.rs     ← LsmEngine（對外 API）
```

依賴關係：`engine` → {`memtable`, `wal`, `sstable`} → `lib.rs (Error)`

---

## 1. `src/lib.rs` —— crate 入口

```rust
pub mod memtable;
pub mod wal;
pub mod sstable;
pub mod engine;

pub use engine::LsmEngine;
```

**Rust 觀念**：
- `pub mod xxx;` 把 `src/xxx.rs` 變成這個 crate 的子模組。
- `pub use engine::LsmEngine` 是「re-export」 —— 讓使用者寫 `use lsm_kv::LsmEngine`，
  不用寫 `use lsm_kv::engine::LsmEngine`。是 API 設計的小細節。

```rust
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Corruption(String),
    KeyNotFound,
}
```

**Rust 觀念**：
- `#[derive(Debug)]` 自動產生 `{:?}` 格式化的能力（可以 `println!("{:?}", err)`）。
- enum 帶資料（algebraic data type）。`Io(std::io::Error)` 表示「IO 錯誤型別會把 std 的 io::Error 包進去」。

```rust
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Error::Io(e) }
}
```

這個 `impl From` 是讓 `?` 運算子能自動把 `io::Error` 轉成我們的 `Error`。
程式碼裡才能寫 `let f = File::open(path)?;` —— `File::open` 回傳 `io::Result<File>`，
`?` 看到 Err 就 `From` 一下變成我們的 `Error` 然後早退。

---

## 2. `src/memtable.rs` —— 寫入緩衝

### LSM 觀念
- MemTable 必須**有序**（讓 flush 出去的 SSTable 自然有序）
- 用 `BTreeMap` 而不是 `HashMap`：HashMap 沒有順序
- `Value::Tombstone` —— 刪除是寫入墓碑，不是真的移除

### Rust 觀念

```rust
pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
    let new_size = key.len() + value.len();
    let old_size = self.map.get(&key).map(|v| key.len() + v.encoded_size()).unwrap_or(0);
    self.approximate_size = self.approximate_size + new_size - old_size;
    self.map.insert(key, Value::Put(value));
}
```

- `&mut self`：可變借用 self（你要修改裡面的 map）
- `Vec<u8>`（不是 `&[u8]`）：表示「我要拿走 key 的所有權，存進 map 裡」
- `self.map.get(&key)`：`&key` 是不可變借用 —— 你只是查，不轉移所有權
- `.map(|v| ...)`：閉包，類似 lambda
- `.unwrap_or(0)`：是 `None` 給 0

```rust
pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &Value)> {
    self.map.iter()
}
```

`impl Iterator<...>` 是「這個函式回傳某個 Iterator，但我不告訴你具體型別是什麼」。
編譯器知道實際型別、會做最佳化，但你不用在簽名裡寫一堆型別噪音。

---

## 3. `src/wal.rs` —— 持久化日誌

### LSM 觀念
- WAL 是 **append-only** —— 從不修改既有內容
- 每筆寫入流程：先 append WAL → fsync → 才改 MemTable
- **CRC32 校驗**：防止「寫到一半斷電」造成重啟時讀到亂掉的資料
- replay 時遇到 CRC 不對就**停下**，前面的紀錄當作有效

### 紀錄格式（再次貼）

```
┌────────┬─────────┬────────────┬────────┬──────┬──────────────┐
│ crc32  │ key_len │ value_len  │ op_tag │ key  │ value (可空) │
│ 4 byte │ 4 byte  │ 4 byte     │ 1 byte │ ...  │ ...          │
└────────┴─────────┴────────────┴────────┴──────┴──────────────┘
```

### 重點程式碼

```rust
fn append_record(&mut self, op: u8, key: &[u8], value: &[u8]) -> Result<()> {
    let mut payload = Vec::with_capacity(4 + 4 + 1 + key.len() + value.len());
    payload.extend_from_slice(&(key.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
    payload.push(op);
    payload.extend_from_slice(key);
    payload.extend_from_slice(value);

    let crc = crc32fast::hash(&payload);
    self.writer.write_all(&crc.to_le_bytes())?;
    self.writer.write_all(&payload)?;
    Ok(())
}
```

**`to_le_bytes()`**：little-endian 編碼。x86 / ARM 都是 little-endian，
讀寫不需要 byte swap。

**先組 payload 再算 crc**：crc 必須涵蓋「除了 crc 自己以外的全部」，
所以先算好整段才能 hash 它。

### `replay` 的精髓：torn write 處理

```rust
match r.read(&mut buf[filled..])? {
    0 => return Ok(false),   // EOF：可能是乾淨結束，可能是 torn write
    n => filled += n,
}
```

讀到一半 EOF（檔案被截斷）就回傳 false，呼叫端跳出迴圈。
這配上 CRC 校驗，讓 WAL 在「最後一筆寫到一半斷電」的情境下能正確還原前面的紀錄。

### sync 的細節

```rust
pub fn sync(&mut self) -> Result<()> {
    self.writer.flush()?;            // BufWriter → OS
    self.writer.get_ref().sync_all()?;  // OS → 磁碟
    Ok(())
}
```

兩階段：
1. `BufWriter::flush()`：把 Rust 層的 buffer 推到 OS。
2. `File::sync_all()`：對應 `fsync(2)` 系統呼叫，要求 OS 把 page cache 同步到磁碟。

兩個都做才算「持久化」。少做一個都會在斷電時丟資料。

---

## 4. `src/sstable.rs` —— 磁碟上的有序檔

### LSM 觀念
- 不可變：寫一次就不再修改
- 有序：條目依 key 升序，所以可以 binary search
- 自帶 index：避免每次查詢都要掃整檔

### 檔案格式

```
┌─────────────── data ───────────────┐
│ entry_0  (crc | key_len | val_len  │
│           | tag | key | value)     │
│ entry_1                            │
│ ...                                │
├─────────────── index ──────────────┤
│ (key_len | key | offset_to_entry)  │
│ ... 每筆 entry 一條 ...             │
├─────────────── footer (16 bytes) ──┤
│ index_offset (8) | num_entries (8) │
└────────────────────────────────────┘
```

### 寫入：`SsTableWriter`

```rust
pub fn append(&mut self, key: &[u8], value: &Value) -> Result<()> {
    if let Some(prev) = &self.last_key {
        assert!(prev.as_slice() < key, "...");
    }
    // ... 寫 entry，記下 (key, offset) 到 self.index
}

pub fn finish(mut self) -> Result<PathBuf> {
    // 把 self.index 一條一條寫到檔案
    // 寫 footer (index_offset, num_entries)
    // fsync
}
```

**為什麼 `finish` 拿 `mut self` 而不是 `&mut self`？**

`self` 是「by value」 —— 拿走整個 struct 的所有權。`finish` 跑完，
這個 writer 就被「消耗」了，呼叫端不能再用它寫東西。
這是 Rust 的常見模式 —— 用型別系統強制「狀態轉移」（writer → 完成）。

### 讀取：`SsTableReader::open`

```rust
pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
    let file = File::open(&path)?;
    let file_len = file.metadata()?.len();
    // ...
    reader.seek(SeekFrom::Start(file_len - FOOTER_SIZE))?;
    let mut footer = [0u8; FOOTER_SIZE as usize];
    reader.read_exact(&mut footer)?;
    let index_offset = u64::from_le_bytes(footer[0..8].try_into().unwrap());
    let num_entries = u64::from_le_bytes(footer[8..16].try_into().unwrap()) as usize;
    // ... 從 index_offset 把整個 index 讀進記憶體
}
```

「seek 到結尾讀 footer，再 seek 到 index_offset 讀 index」 —— 這是
所有「附帶 metadata」格式檔案的標準作法（ZIP、Parquet 也是）。
為什麼 footer 在尾巴而不是 header？因為**寫入時 index 大小未知**，header 沒辦法填好；
寫到最後再 append 一個固定大小的 footer 反而簡單。

### binary search

```rust
pub fn get(&self, key: &[u8]) -> Result<Option<Value>> {
    let pos = self.index.binary_search_by(|(k, _)| k.as_slice().cmp(key));
    let offset = match pos {
        Ok(i) => self.index[i].1,
        Err(_) => return Ok(None),
    };
    // ... seek 到 offset 讀那一筆
}
```

`binary_search_by` 回傳：
- `Ok(i)`：找到，i 是位置
- `Err(i)`：沒找到，i 是「如果要插入會在哪」（這裡用不到，直接回 None）

---

## 5. `src/engine.rs` —— 串起來的 LsmEngine

### LSM 觀念集大成

**寫入路徑**：

```rust
pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
    self.wal.append_put(key, value)?;
    self.wal.sync()?;
    self.memtable.put(key.to_vec(), value.to_vec());
    self.maybe_flush()?;
    Ok(())
}
```

**順序很關鍵**：
1. WAL 先 append 並 fsync —— 確保「告訴使用者成功之前資料已落地」
2. 再更新 MemTable —— 讓 get 看得到
3. 最後檢查是否該 flush

如果先改 MemTable 再寫 WAL，那 WAL 寫到一半斷電時，
重啟還原 MemTable 會少最後一筆，但「使用者已經看到我們回傳成功了」 —— 違反持久性。

**讀取路徑**：

```rust
pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
    if let Some(v) = self.memtable.get(key) {
        return Ok(materialize(v.clone()));
    }
    for sst in self.sstables.iter().rev() {  // ← rev！從新到舊
        if let Some(v) = sst.get(key)? {
            return Ok(materialize(v));
        }
    }
    Ok(None)
}

fn materialize(v: Value) -> Option<Vec<u8>> {
    match v {
        Value::Put(bytes) => Some(bytes),
        Value::Tombstone => None,        // ← 看到墓碑當作不存在
    }
}
```

兩個重點：
- `.iter().rev()`：reverse iterator，從最新的 SSTable 開始查
- `materialize(Tombstone)` → `None`：墓碑被當「不存在」回給使用者，
  也阻止繼續往下查（`return Ok(None)` 提早結束 for loop）

### Flush 流程

```rust
fn do_flush(&mut self) -> Result<()> {
    let id = self.next_sst_id;
    let path = self.dir.join(format!("{:020}.sst", id));

    let mut writer = SsTableWriter::create(&path)?;
    for (k, v) in self.memtable.iter() {
        writer.append(k, v)?;
    }
    let path = writer.finish()?;       // 內部 fsync

    let reader = SsTableReader::open(&path)?;
    self.sstables.push(reader);
    self.next_sst_id += 1;

    self.memtable = MemTable::new();    // 清 MemTable
    self.wal.truncate()?;               // 清 WAL
    Ok(())
}
```

**順序很關鍵（再一次）**：
- 一定要先確認 SSTable 安全落地（finish 內 fsync），**才**清 MemTable + WAL。
- 反過來做：清完 WAL 後 SSTable 寫到一半崩潰 → 兩邊都沒了 → 資料遺失。

### 開檔（重啟還原）

```rust
pub fn open_with_threshold<P: AsRef<Path>>(dir: P, flush_threshold: usize) -> Result<Self> {
    fs::create_dir_all(&dir)?;

    // 1. 掃目錄，找出所有 .sst 檔
    let mut sst_paths: Vec<(u64, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        // 解析 "00000000000000000007.sst" 這種檔名
    }
    sst_paths.sort_by_key(|(id, _)| *id);

    let next_sst_id = sst_paths.last().map(|(id, _)| id + 1).unwrap_or(0);
    let sstables = sst_paths.into_iter()
        .map(|(_, p)| SsTableReader::open(p))
        .collect::<Result<Vec<_>>>()?;

    // 2. replay WAL → 重建 MemTable
    let wal_path = dir.join("wal.log");
    let memtable = Wal::replay(&wal_path)?;
    let wal = Wal::open(&wal_path)?;

    Ok(Self { dir, memtable, wal, sstables, next_sst_id, flush_threshold })
}
```

兩步：
1. 掃出所有 SSTable，由舊到新放進 vector
2. WAL replay 把上次崩潰前在 MemTable 但還沒 flush 的紀錄還原

正確性：因為「flush 完才清 WAL」，所以 WAL 裡的紀錄一定都是「沒進到 SSTable」的。
重播一遍剛好恢復原狀。

---

## 6. `src/main.rs` —— CLI

沒什麼神奇的 —— 解析 `std::env::args()` 然後呼叫 `LsmEngine` 對應方法。
重點看 `bench` 那段，方便你之後做性能優化時對照前後差異。

---

## 7. 怎麼讀完這份程式碼

建議順序：

1. 跑一次：`cargo test`，確認 16 個測試都過。
2. 看 [01-lsm-tree-concepts.md](01-lsm-tree-concepts.md) 把觀念架構先建好。
3. 翻 `memtable.rs`（最簡單）→ `wal.rs` → `sstable.rs` → `engine.rs`。
   每個檔案配對 [02-rust-quickstart.md](02-rust-quickstart.md) 對應的語法觀念。
4. 用 CLI 玩：put、get、flush、再 put、再 get，觀察 `data/` 目錄裡多了什麼檔。
5. 試試破壞 `data/wal.log` 最後幾個 byte，看重啟會不會少最後一筆。
6. 挑 [04-next-steps.md](04-next-steps.md) 一個方向動手寫。
