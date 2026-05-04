# LSM-Tree 核心概念

> 這份文件不講 Rust 語法，只講 LSM-Tree 本身。讀完你應該能跟人解釋：
> 為什麼 RocksDB / LevelDB / TiKV / Cassandra / HBase 都選這個架構。

## 1. 一句話總結

> **LSM-Tree（Log-Structured Merge Tree）用「寫入永遠是 append」的方式換取磁碟友善的順序 IO，
> 代價是讀取時可能要看好幾個地方。**

接下來我們一層一層拆解這句話。

---

## 2. 為什麼需要新的儲存結構？B-Tree 哪裡不好？

傳統 RDBMS（MySQL InnoDB、PostgreSQL）用 **B+Tree** 當索引結構。B+Tree 寫入流程是：

1. 找到 key 該屬於的 leaf page
2. **就地修改** 那個 page
3. 把修改後的 page 寫回磁碟

問題出在第 3 步。假設你每秒寫入 10 萬個 key，這 10 萬個 key **散落在不同的 page**。
每次 page 變動都會產生一筆「對某個磁碟位置的隨機寫入」。

| 操作 | HDD 大約速度 | SSD 大約速度 |
|---|---|---|
| 順序讀寫 | 100~200 MB/s | 1~5 GB/s |
| 隨機讀寫（4KB） | **~1 MB/s** | ~100~500 MB/s |

HDD 的隨機 IO 比順序 IO **慢了一兩百倍**。SSD 雖然差距小，但**寫入放大（write amplification）**
仍然是 SSD 壽命的殺手 —— SSD 的 erase block 通常 4MB，你只想改 4KB 也得搬整個 block。

> **結論：B-Tree 為了「讀取快、就地更新」的優點，付出「寫入隨機 IO」的代價。
> 在「寫入大於讀取」的工作負載（log、IoT、metrics、事件流）下，這個權衡是賠錢的。**

LSM-Tree 反過來：**寫入永遠 append**，讀取時想辦法解決。

---

## 3. LSM-Tree 的基本原理

### 3.1 三個關鍵元件

```
寫入
  │
  ▼
┌─────────────┐         ┌──────────────┐
│   MemTable  │ ◀─────  │     WAL      │  ← append-only 日誌（持久化保證）
│  (在 RAM)    │         │  (在 disk)   │
└─────┬───────┘         └──────────────┘
      │ 滿了 → flush
      ▼
┌─────────────┐
│  SSTable_3  │ ← 最新
├─────────────┤
│  SSTable_2  │
├─────────────┤
│  SSTable_1  │ ← 最舊
│   (磁碟上)   │
└─────────────┘
      ▲
      │
讀取  │  ← 從 MemTable 開始，找不到再往下層找
```

**MemTable**：記憶體中的有序資料結構。所有寫入先進這裡。
RAM 寫入沒有隨機 IO 的代價，可以單機每秒幾十萬筆。

**WAL（Write-Ahead Log）**：磁碟上的 append-only 日誌。
每筆寫入「先 append 到 WAL，再更新 MemTable」。
這樣斷電時，重啟讀 WAL 就能還原 MemTable。

**SSTable（Sorted String Table）**：磁碟上**已排序、不可變**的資料檔。
當 MemTable 滿（例如 64MB），整批 flush 成一個 SSTable，然後 MemTable 清空。
注意：**這次磁碟寫入是純粹的順序寫**，因為 MemTable 本身就是排序的，
所以 flush 出去的 SSTable 也天生有序。

### 3.2 寫入路徑（put / delete）

```
1. fsync(WAL.append(record))    ← 持久化
2. MemTable.insert(key, value)  ← 記憶體更新
3. 如果 MemTable 滿了：
     a. 把 MemTable 凍結
     b. 順序寫成新 SSTable
     c. fsync 那個 SSTable
     d. 清空 MemTable，截斷 WAL
```

**寫入只有兩種磁碟動作**：
- 一筆 append 到 WAL（很短，通常幾百 byte）
- 偶爾的順序 flush 寫一個 SSTable（一次 64MB 連續寫）

兩者都是 **磁碟最擅長的順序 IO**。

### 3.3 讀取路徑（get）

```
1. 先查 MemTable。找到 → 結束。
2. 從新到舊查每一個 SSTable。第一個找到的就回傳。
3. 全部找不到 → key 不存在。
```

「從新到舊」很關鍵：同一個 key 可能在多個 SSTable 都有（因為被多次更新），
**最新的版本必須贏**。

### 3.4 怎麼刪除？—— Tombstone（墓碑）

SSTable 是**不可變的**。寫一次就不再修改。所以「刪除某個 key」**不能去 SSTable 裡找出來抹掉**。

LSM 的解法：**寫一個「墓碑」記錄**。

```
delete("k")  →  在 MemTable 寫入 (k, Tombstone)
                之後 flush 時，墓碑也會進到 SSTable。
```

讀取時：
```
get("k")
  → MemTable 找到 Tombstone → 回傳「不存在」（不再往下查 SSTable！）
  → 即使 SSTable_1 裡還有 (k, "v")，也會被新墓碑蓋過。
```

真正的清理在 **compaction** 階段：合併 SSTable 時，遇到 tombstone 就把那個 key 的所有舊版本都丟掉。

### 3.5 為什麼這樣寫入會比較快？

| 動作 | B-Tree | LSM-Tree |
|---|---|---|
| 寫入定位 | 隨機 IO 找 leaf page | RAM 中的 MemTable，無 IO |
| 寫入磁碟 | 隨機寫一個 page | append WAL（順序）|
| 後續寫入 | 又一輪隨機 IO | append WAL，MemTable 累積 |
| 批次落盤 | —— | flush 成 SSTable（順序寫）|

LSM 把「N 筆隨機 IO」轉成了「N 筆 WAL append + 一次大塊順序寫」。
順序 IO 的成本與隨機 IO 差兩個數量級，這就是吞吐優勢的來源。

---

## 4. 必須付出的代價

天下沒有白吃的午餐。LSM 換來的問題：

### 4.1 讀取放大（Read Amplification）

要找一個 key，最壞情況要查 1 個 MemTable + N 個 SSTable。SSTable 越多，讀越慢。

**緩解手段**：
- **Bloom Filter**：每個 SSTable 配一個 bloom，先問 bloom「key 可能在嗎？」，
  bloom 說不在就跳過這個 SSTable（bloom 不會偽陰性）。
- **Block cache**：常用的 SSTable block 快取在 RAM。
- **Compaction**：定期合併 SSTable，減少數量。

### 4.2 空間放大（Space Amplification）

同一個 key 可能在多個 SSTable 都有副本（最新的才是有效的，舊的是垃圾）。
Tombstone 也佔空間。Compaction 之前，磁碟可能是真實資料量的數倍。

### 4.3 寫入放大（Write Amplification）

聽起來很矛盾：「LSM 不是寫入很快嗎？怎麼又有寫入放大？」
這裡的放大指的是 **compaction 過程**：合併 SSTable 時，舊資料會被讀出來、與新資料合併、寫成新檔。
同一筆資料在它的「壽命」裡可能被搬動好幾次。

工業級系統用 **Leveled Compaction** 或 **Tiered Compaction** 來控制這個放大係數。

---

## 5. Compaction 策略（這版實作沒做，但要知道）

### 5.1 Tiered Compaction（Cassandra 風格）

把 SSTable 分層，每層的「容量」是上一層的 K 倍。每層滿了就把那層所有檔案合併，
生出一個檔案到下一層。

- 優點：寫入放大低（一筆資料寫入磁碟次數少）
- 缺點：空間放大大、讀取放大也大（每層都要查）

### 5.2 Leveled Compaction（LevelDB / RocksDB 預設）

- L0 比較特殊：每個 L0 SSTable 之間 key range 可能重疊（直接從 MemTable flush 而來）。
- L1 以下：每層內所有 SSTable 的 key range 互不重疊。
- L1 容量 10MB，L2 100MB，L3 1GB...
- 當 L_n 超出容量，挑一個 SSTable 跟 L_(n+1) 中 key range 重疊的所有 SSTable 合併。

- 優點：讀取放大低（L1 以下每層只需查一個檔）、空間放大低
- 缺點：寫入放大高

**實務上**：RocksDB 預設用 Leveled，因為大部分線上服務讀多寫少。寫入很重的場景才考慮 Tiered。

---

## 6. 持久性（Durability）的細節：fsync 才是真的

新手最容易踩的坑：**「我寫到檔案了，斷電應該不會丟吧？」**

實際上：

```
write(fd, buf, n)     ← 資料只到 OS page cache（在 RAM）
fsync(fd)             ← OS 把 page cache 同步到磁碟
```

只 `write` 不 `fsync`，斷電會丟。我們的 WAL 在每次 `put` 之後都呼叫 `sync()`，
代價就是 benchmark 看到的 252 ops/sec。

> **macOS 的暗坑**：`fsync(2)` 在 macOS 上**不**真的把資料同步到磁碟硬體 ——
> 只到 disk 的 write cache。要真的同步，得呼叫 `fcntl(fd, F_FULLFSYNC)`。
> 這個專案為了學習目的不處理這個細節，但這是你準備系統廠面試的好題目。

### Group Commit：吞吐優化的標準作法

每筆 put fsync 一次太慢。實務上會：
1. 寫入時 append WAL，但**不立刻 fsync**。
2. 一個背景 thread 每 N ms 或累積 M 筆後做一次 fsync。
3. fsync 完才回應使用者「寫入成功」。

代價是「latency 變高一點」（要等到下一次 fsync 才能 ack），但「throughput 大幅提升」（fsync 攤分到多筆寫入）。
這是教科書級別的「latency vs throughput」權衡。

---

## 7. 它在哪些系統裡被用？

| 系統 | 用途 | 注解 |
|---|---|---|
| **LevelDB** | 嵌入式 KV | Google 開源，LSM-Tree 的「教科書實作」|
| **RocksDB** | 嵌入式 KV | Facebook fork LevelDB，工業級 |
| **TiKV** | 分散式 KV | TiDB 底層，Rust 寫，內部用 RocksDB |
| **Cassandra** | 分散式寬列 | 早期 LSM 的代表 |
| **HBase** | 分散式寬列 | Hadoop 生態 |
| **ScyllaDB** | Cassandra 相容 | C++ 重寫，效能更高 |
| **InfluxDB** | 時序資料庫 | TSI 索引 + LSM 變種 |
| **Kafka** | 訊息佇列 | log-structured，思想相同 |
| **etcd** / **Consul** | 共識系統 | bbolt 是 B-Tree，但 WAL 思想是 LSM |

---

## 8. 這份實作對應到上面的哪些觀念？

讀完這篇後，可以回去看程式碼，對應關係：

| 觀念 | 程式碼位置 |
|---|---|
| MemTable（有序緩衝） | [src/memtable.rs](../src/memtable.rs) |
| Tombstone | `Value::Tombstone` enum |
| WAL append + crc | [src/wal.rs](../src/wal.rs) `Wal::append_record` |
| WAL replay（torn write 處理）| [src/wal.rs](../src/wal.rs) `Wal::replay` |
| SSTable 寫入 | [src/sstable.rs](../src/sstable.rs) `SsTableWriter` |
| SSTable binary search | [src/sstable.rs](../src/sstable.rs) `SsTableReader::get` |
| 「從新到舊查 SSTable」 | [src/engine.rs](../src/engine.rs) `LsmEngine::get` |
| flush 觸發條件 | [src/engine.rs](../src/engine.rs) `maybe_flush` |
| 重啟時掃目錄 + replay WAL | [src/engine.rs](../src/engine.rs) `LsmEngine::open` |

---

## 9. 進一步閱讀（建議順序）

1. **LevelDB 原始 paper**：<https://github.com/google/leveldb/blob/main/doc/index.md>
   先看 LevelDB 文件，比 paper 好讀。
2. **The Log-Structured Merge-Tree (O'Neil et al., 1996)**：原始論文。
3. **RocksDB Wiki**：<https://github.com/facebook/rocksdb/wiki>
   工業級實作的所有細節。
4. **Designing Data-Intensive Applications**（Kleppmann）第 3 章：
   是這類書裡最清楚的 LSM 介紹。
5. **TiKV Source Reading**：<https://tikv.org/deep-dive/key-value-engine/introduction/>
   Rust 實作 + 分散式擴充，跟你這個專案的下一步高度重疊。
