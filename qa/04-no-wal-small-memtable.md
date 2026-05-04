# Q04：取消 WAL + 縮小 MemTable 行得通嗎？

## 我的提問（自問自答的推導）

> 能用是一定能用，而且還會增加效能。
> 但問題就是要承擔 memtable 遺失風險。
> 所以這可以變化 —— 把 memtable 設定小 chunk，遺失有限。
> 但這路的成本就是背景要有效整理 sstable，不然會有太多 chunk。

## 結論：你自己推導出工業級的設計權衡

這條推導完全正確，而且有正式名稱 —— 這就是 **Tiered Compaction Strategy** 的設計哲學。

```
取消 WAL → 寫入更快（少一次磁碟 IO）
        → MemTable 崩潰會丟
        → 縮小 MemTable，丟失上限變小
        → 但 MemTable 縮小 → flush 變頻繁 → SSTable 變多變小
        → 必須有更積極的 compaction 把碎檔合併
```

## Tiered vs Leveled

| 策略 | MemTable 大小 | Flush 頻率 | SSTable 數量 | Compaction 強度 | 使用者 |
|---|---|---|---|---|---|
| Leveled（保守） | 大（64MB+）| 低 | 少 | 強（持續整理） | LevelDB / RocksDB 預設 |
| Tiered（你的方向）| 小 | 高 | 多 | 弱（讓它累積）| Cassandra 預設 |
| 無 WAL + 小 MemTable | 極小 | 極高 | 極多 | 極強 | 純快取 / metrics |

不是哪個對哪個錯，是**不同 workload 的最佳解**。

## RUM Conjecture：通用三角權衡

任何儲存引擎都只能優化其中兩個：

```
                  Read (讀取放大)
                       △
                      ╱ ╲
                     ╱   ╲
                    ╱     ╲
                   ╱       ╲
       Update ◀──╱─────────╲──▶ Memory/Storage
       (寫入放大)              (空間放大)
```

你的設計（小 MemTable + 無 WAL + 強 compaction）落在這裡：

```
                  Read 放大 ↑↑   ← SSTable 變多 → 讀變慢
                       △
                      ╱ ╲
                     ╱   ╲
                    ╱     ╲
                   ╱       ╲
       Update ↓ ──╱─────────╲──▶ Space ↓
       (寫入超快)              (compaction 控空間)
```

**你優化了寫入和空間，代價是讀取變慢。**
適合：寫多、讀少、可丟資料 → metrics、log 收集、IoT 感測器。
這正是 InfluxDB、Cassandra 早期版本的設計。

## 我原本沒想到的兩個隱含後果

### 隱含後果 1：寫入放大會「反彈」

直覺：「縮小 MemTable → 寫入更快」 —— **短期對，長期錯。**

```
大 MemTable（64MB）：
  64MB 在記憶體合併寫入
  → flush 1 個 64MB SSTable
  → 同 key 多次更新只寫 1 次到 disk
  → 寫入放大 = 1x

小 MemTable（1MB）：
  1MB 就 flush
  → 64 個 1MB 小 SSTable
  → 同 key 散在多個檔案
  → compaction 反覆讀寫合併
  → 寫入放大 5x ~ 10x
```

**結果**：
- 應用層看到的寫入 ops/sec 變高 ✅
- 但磁碟實際寫入的 byte 數變多 ❌
- SSD 壽命被吃掉（NAND flash 寫入次數有限）

→ 工業界最常踩的坑之一。

### 隱含後果 2：讀取變慢比想像中嚴重

「SSTable 多 → 讀取要看更多檔」這層我已經想到。但有放大效應：

```
讀一個 key 的成本：
  
  1 個 SSTable：       1 次 binary search
  10 個 SSTable：     10 次 binary search
  100 個 SSTable：    100 次 binary search    ← cache miss 開始發生
  1000 個 SSTable：   每個都查 → 磁碟 IO 風暴
```

**而且**：每多一個 SSTable，就多一份 bloom filter / index 在 RAM。
SSTable 太多，光 metadata 就吃光記憶體。

緩解：用 **Bloom Filter** 快速判定「key 一定不在這個 SSTable」，避免無謂 IO。
**Tiered 策略下 Bloom Filter 的價值比 Leveled 高得多**，因為 SSTable 多得多。

## 完整的 LSM 配置決策樹

```
寫多還是讀多？
├── 寫多
│   └── 資料能丟嗎？
│       ├── 能丟 → 關 WAL + 小 MemTable + Tiered compaction
│       │         （我推導的這條）
│       │         例：metrics、log、事件流
│       │
│       └── 不能丟 → 開 WAL + Group commit + 中型 MemTable
│                  例：訂單、交易記錄
│
└── 讀多
    └── 大 MemTable + Leveled compaction + Bloom filter + Block cache
         例：使用者資料、商品目錄
         （RocksDB 預設配置）
```

## 我學到了什麼

我對 LSM 的理解已經從「會寫」進階到「會調優」。
這之間的差距是工程師最值錢的部分 —— 會寫的人一堆，
**會根據 workload 配置 LSM 參數的人很少**。

下一步：去 [docs/04-next-steps.md](../docs/04-next-steps.md) §1.2 (Compaction)、§1.1 (Bloom Filter)
動手做，能在程式碼上實際量出 Leveled vs Tiered、有 bloom vs 無 bloom 的差異。
這比讀任何 paper 都有感。
