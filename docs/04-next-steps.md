# 下一步：把這個玩具改造成真正的儲存引擎

> 這個專案故意把功能砍到最小，方便一次理解全貌。讀完程式碼之後，
> 下面的方向是循序漸進的進階挑戰。每一個都對應「系統廠 / 資料庫公司」面試會考的核心題目。

按建議的學習順序排：

---

## Level 1：先把基本功補完

### 1.1 Bloom Filter（**最值得先做**）

**問題**：目前 `LsmEngine::get` 對找不到的 key 會把每個 SSTable 都查一次。SSTable 越多，miss 越貴。

**做法**：每個 SSTable 配一個 bloom filter（幾 KB 的 bit array）。
- 寫入 SSTable 時，把每個 key hash 進 bloom。
- 讀取時，先問 bloom：「key 可能在嗎？」
  - 說「不在」→ 100% 不在，直接跳過。
  - 說「在」→ 才真的 binary search index。

**學到什麼**：機率資料結構、false positive rate 計算、磁碟 IO 觀念。

**實作提示**：bloom filter 大小公式 `m = -n * ln(p) / (ln(2)^2)`（n = key 數，p = 期望 false positive 率）。
把 bloom 寫在 SSTable 的 footer 之前、index 之後就好。

### 1.2 Compaction（這是 LSM 的靈魂）

**問題**：SSTable 數量無限增長，讀取與磁碟空間都會爆。

**做法**：實作一個簡單的 `compact()` 方法：
- 取最舊的 N 個 SSTable
- 用「k 路歸併」（heap 排序）把它們合併成一個新 SSTable
- 過程中：同一個 key 多版本只保留最新；遇到 tombstone 就把該 key 整個丟掉
- 寫完新檔後，把舊檔 unlink

進階：實作 Leveled Compaction（每層大小不同、key range 不重疊）。

**學到什麼**：歸併排序的應用、原子性檔案操作（怎麼避免合併到一半崩潰造成資料遺失）。

### 1.3 Block-based SSTable

**問題**：目前 SSTable 的 index 是「每個 entry 一條」，幾百萬筆 key 會讓 index 占很多記憶體。

**做法**：把 data 切成固定大小的 block（典型 4KB）。
- 每個 block 只在 index 裡記一條（block 內第一個 key + offset）。
- 讀取時：binary search index 找到 block，把整個 block 讀進來，block 內再線性掃描。

**學到什麼**：磁碟 IO 是以 block 為單位的（4KB / 16KB），這個架構讓「一次磁碟 IO 換多筆 entry」。

### 1.4 真實的 fsync（macOS 特別篇）

`fsync(2)` 在 macOS 上**不**真的刷到磁碟硬體。要做需要：

```rust
use std::os::fd::AsRawFd;
unsafe {
    libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC);
}
```

**學到什麼**：作業系統與磁碟之間還有好幾層 cache。「持久化」沒你想的那麼簡單。

---

## Level 2：讓它跑得快

### 2.1 Group Commit（吞吐殺手鐧）

目前每筆 put 都 fsync，benchmark 只有 252 ops/sec。改成：

- 寫入 thread 把記錄推到一個 channel，立刻回傳「成功」（暫時假成功）
- 一個 background thread 從 channel 收記錄，累積到 N 筆或等 M ms 後做一次 fsync
- fsync 完才透過 callback / oneshot channel 通知所有等待的寫入「真的成功了」

**學到什麼**：channel-based async、latency vs throughput 的權衡、commit log 設計。

預期效果：吞吐從幾百 ops/sec 拉到幾萬 ops/sec。

### 2.2 mmap 讀 SSTable

目前讀取 SSTable 用 `seek + read`，每次都走系統呼叫。改用 `memmap2` crate：
- SSTable 整個 mmap 進記憶體，OS 自動管理 paging
- 讀取就是 RAM 存取

**學到什麼**：虛擬記憶體、page fault、為什麼 OS 比應用層更會做 cache。
**注意**：mmap 在大檔上有 page fault 抖動的問題，所以 RocksDB 預設不用 mmap，這是有取捨的。

### 2.3 並發讀

目前 API 全是 `&mut self`，一次只能一個 thread。改成：
- MemTable 換成 `crossbeam_skiplist::SkipMap`（lock-free）
- SSTable 列表用 `Arc<Vec<Arc<SsTableReader>>>`，更新時做 copy-on-write
- 寫入用 mutex 保護，讀取完全無鎖

**學到什麼**：Rust 的 `Arc` / `RwLock` / lock-free 資料結構、Read-Copy-Update 模式。

---

## Level 3：分散式

### 3.1 Raft + LSM = 迷你版 etcd / TiKV

把這個 KV 引擎包進 [`raft-rs`](https://github.com/tikv/raft-rs)：
- 寫入要先過 Raft log replication 才提交到 LSM
- 讀取走 leader（強一致）或任何 replica（最終一致）
- WAL 與 Raft log 可以共用一份（這是 etcd 的設計）

**學到什麼**：分散式共識、CAP、leader election、log replication。
這直接對標 TiKV / etcd 的核心，是「分散式系統工程師」的標準題目。

### 3.2 Range Sharding

當資料量超過單機，把 key space 切成 range，每個 shard 一個 LSM 實例。
shard 之間做 split / merge，並用 metadata service（PD in TiKV）管理 placement。

**學到什麼**：sharding 策略、re-balance、hot spot 處理。

---

## Level 4：把 SQL 接上來

### 4.1 KV → Key/Value 編碼

要在 KV 上跑 SQL，先要把「行 + 列 + 索引」編碼成 KV：
- 主鍵：`tablePrefix + tableId + rowId` → row data
- 二級索引：`indexPrefix + indexId + indexedColumn + rowId` → empty
- 範圍掃描：`prefix_iter()` 找一個 prefix 下所有 KV

**學到什麼**：MyRocks / TiDB 的編碼方案、為什麼 ordered KV 是 SQL 引擎的好底層。

### 4.2 簡易 SQL parser + executor

用 [`sqlparser-rs`](https://github.com/sqlparser-rs/sqlparser-rs) parse SQL，
寫一個 volcano-style 執行器：每個運算子是一個 iterator，組合成執行計畫。

**學到什麼**：query planning、physical operators、optimizer 入門。

---

## Level 5：往下挖到系統層

### 5.1 用 io_uring 取代 read/write

Linux 5.x 之後的非同步 IO 介面 `io_uring` 可以做到「一次 syscall 提交一批 IO」。
Rust 用 [`tokio-uring`](https://github.com/tokio-rs/tokio-uring) 或 [`glommio`](https://github.com/DataDog/glommio)。

**學到什麼**：Linux 高效能 IO 的最前沿、ring buffer 的設計。
**警告**：macOS 沒有 io_uring，要在 Linux VM / Asahi 上做。

### 5.2 自己寫一個 skip list 取代 BTreeMap

MemTable 用 BTreeMap 是為了簡單。手寫一個 skip list 會學到：
- 機率資料結構
- 不需要 rebalance 的有序結構（B-Tree / Red-Black tree 的替代）
- 為什麼 LevelDB / RocksDB / Redis sorted set 都用 skip list

### 5.3 用 SIMD 加速 CRC

`crc32fast` crate 已經用 SIMD 了（CLMUL 指令）。但你可以自己用 `std::simd` 寫一個試試，
比較跟原生實作的差距。

**學到什麼**：SIMD intrinsics、CPU 微架構、為什麼現代 hash / 加密 / 壓縮都靠 SIMD。

---

## 路徑建議

**如果你想銜接系統廠面試**：1.1 → 1.2 → 2.1 → 5.2。
這條路徑會讓你能在白板上講清楚 RocksDB 的內部運作，並且實際寫過。

**如果你想做分散式**：1.1 → 1.2 → 2.3 → 3.1。
做完你就有個迷你版 etcd 可以放履歷。

**如果你想做資料庫**：1.1 → 1.3 → 4.1 → 4.2。
做完你會懂 TiDB / CockroachDB 的底層設計。

每個方向都夠寫一篇 blog post。把過程寫下來、放 GitHub，會比 100 個 leetcode 對求職更有說服力。
