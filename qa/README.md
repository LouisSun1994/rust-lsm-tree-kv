# Q&A 學習紀錄

這個資料夾保留我學 LSM-Tree 過程中問過、且**自己推導出有意義結論**的問題。

每一題的格式：
- 我的提問（盡量保留原文，包含當下還不完整的理解）
- 回應的精華（不是全文，只挑「我看回去能持續學到東西」的部分）
- 一句話收尾：這題我學到了什麼

不放純粹操作性問題（例如「幫我 commit」）。

## 目錄

| # | 主題 | 對應觀念 |
|---|---|---|
| [01](01-lsm-essence.md) | LSM 本質：mem block + save disk？ | LSM 骨架的 4 個關鍵細節 |
| [02](02-memtable-sorted-not-hashmap.md) | 為什麼 MemTable 要有序？ | BTreeMap vs HashMap、為什麼有序是 LSM 的前提 |
| [03](03-wal-fsync-tradeoff.md) | WAL 是同步 append？瓶頸在 fsync 頻率？ | latency / throughput / durability 三角權衡 |
| [04](04-no-wal-small-memtable.md) | 取消 WAL + 縮小 MemTable 行得通嗎？ | Tiered vs Leveled、寫入放大反彈、RUM Conjecture |
| [05](05-query-cost-and-bloom-filter.md) | LSM 查詢複雜度為何可控？Bloom Filter 為何可信？ | log N、Bloom 單向誤差、4 個救命手段、點查 vs 範圍查 |
| [06](06-why-rust-not-c-cpp.md) | 為什麼推薦 Rust 而不是 C/C++？ | 編譯期回饋、memory safety、Cargo、職涯差異化、寫 Rust 讀 C |

## 怎麼回顧

建議週期性（例如每兩週）做一次：
1. 從最早的題目讀回來，看每題自己當下的提問
2. 蓋住「答案」那段，先試著自己回答一次
3. 對照看哪些細節還沒內化、哪些已經變成本能

學習的訊號是「**我以前覺得這題很難，現在覺得自然**」。
如果一題回看還是卡在同樣的點，那一塊就是該再深入的方向。
