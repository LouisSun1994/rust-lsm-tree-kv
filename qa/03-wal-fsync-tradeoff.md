# Q03：WAL 是同步 append？瓶頸在 fsync 頻率？

## 我的提問

> WAL 看起來就是寫 disk 吧
> 而且 memtable 跟 WAL 應該是同步 append
> WAL 只是一個防斷電機制對吧？
>
> 而且這邊的瓶頸在於 WAL 多久 fsync 一次
> 不然 append 都還只是在 buffer 中
> 這就是文中說到的吞吐量的權衡
>
> 所以核心概念上
> 要低延遲就是 WAL i/o 次數要少
> 相對當斷電時候 遺失的數據就比較多

## 結論：你完全抓對了 LSM 持久化的精髓

### ✅ 三個正確的觀察

#### 1. 「MemTable 跟 WAL 同步 append」 對

順序是：
```
1. WAL.append(record)    ← 先寫日誌
2. (可能 fsync)
3. MemTable.put(k, v)    ← 再更新 RAM
```

**為什麼先 WAL 後 MemTable？**
反過來會出事 —— MemTable 已更新、使用者 get 看得到，但 WAL 還沒寫，
這時斷電 → 資料消失但你之前已經承諾「寫入成功」 → 違反持久性。

#### 2. 「WAL 只是防斷電機制」 對

WAL 平時不參與讀取，只在「重啟」這一個時刻有用。
讀取流程只看 MemTable + SSTable，從不查 WAL。

#### 3. 「瓶頸在 fsync 頻率」 對

實測證據：
```
每筆 put 都 fsync：
  寫 50000 筆 → 198 秒 → 252 ops/sec

如果每 1000 筆才 fsync 一次：
  → 預期 50,000+ ops/sec
```

## 三角權衡正規化（Group Commit）

| 策略 | fsync 頻率 | 吞吐 | 斷電丟失 | 用在哪 |
|---|---|---|---|---|
| 每筆 fsync | 每次寫入 | 低（百級 ops/s）| 0 | 銀行交易、訂單 |
| 批次 fsync | 每 N 筆 / M ms | 高（萬級 ops/s）| 最後 N 筆 / M ms | 多數線上服務 |
| 完全不 fsync | 永不（依賴 OS）| 最高（百萬級）| 大量 | 快取、可重建資料 |

工業級對應：
- MySQL `innodb_flush_log_at_trx_commit` 0/1/2
- PostgreSQL `synchronous_commit` on/off
- RocksDB `manual_wal_flush`

**它們本質都在做同一件事：讓使用者選擇 fsync 頻率。**

## 補充：buffer 不是一層，是三層

我原本只說「append 還在 buffer」，但 buffer 其實有多層：

```
write(...) 之後資料的旅程：

  應用層
    │
    ▼
  [Rust BufWriter]      ← 第 1 層 (process RAM)
    │  flush()
    ▼
  [OS page cache]       ← 第 2 層 (kernel RAM)
    │  fsync()
    ▼
  [disk cache]          ← 第 3 層
    │  F_FULLFSYNC (macOS)
    ▼
  磁性 / NAND flash     ← 真持久
```

**斷電會不會丟，看你停在哪一層**：

| 停在哪 | 行程 crash 丟？ | 整機斷電丟？ |
|---|---|---|
| BufWriter | **會** | **會** |
| OS page cache | 不會 | **會** |
| disk cache | 不會 | 看 disk 有無電容 |
| 真磁碟 | 不會 | 不會 |

實務含義：
- **行程 crash**（panic、被 kill）→ `flush()` 到 OS 就夠了
- **整機斷電**（拔電源）→ 必須 `fsync()` 到磁碟

Group commit 承擔的風險其實是「整機斷電」，行程 crash 不會丟。

## 對應的程式碼

[src/wal.rs:80](../src/wal.rs:80) 的 `sync()` 同時做了：
```rust
self.writer.flush()?;            // BufWriter → OS
self.writer.get_ref().sync_all()?;  // OS → 磁碟（fsync）
```

兩階段都做才算「真持久化」。

## 我學到了什麼

「為什麼資料庫設定裡有 sync_commit / flush_log_at_trx_commit 這些選項」 —— 
它們不是奇怪的調優旋鈕，是**這個三角權衡的具體化**。
能跟人解釋清楚這個權衡，已經是中高階 backend / DBA 的水準。
