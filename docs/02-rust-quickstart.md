# Rust 快速入門：你會在這專案看到的東西

> 假設你會寫至少一種語言（Java / Go / Python / TypeScript 都行）。
> 這篇不講「Rust 完整語法」，只講你看這個專案的程式碼會碰到的觀念。

## 1. 心智模型：從 GC 語言切換過來

如果你習慣 Java / Go / Python，最大的轉變是：

> **Rust 沒有 GC（垃圾回收器）。記憶體誰擁有、誰釋放，編譯器在編譯時期幫你算清楚。**

這個機制叫 **ownership（所有權）**。它會強迫你在寫程式時就想清楚一件事：
「這塊資料到底是誰負責清理？」

副作用是：
- 學習曲線陡峭（前 1~2 週你會跟編譯器打架）
- 一旦能編譯通過，**幾乎不會有 null pointer、data race、use-after-free** 這類 bug
- 不需要 GC pause，效能可預期（這是金融、遊戲、嵌入式選 Rust 的關鍵）

---

## 2. 三個必懂概念：Ownership、Borrow、Lifetime

### 2.1 Ownership（所有權）

每個值在任何時刻**只有一個 owner**。owner 離開作用域時，值就被釋放（呼叫 `drop`）。

```rust
fn main() {
    let s = String::from("hello");  // s 是這個 String 的 owner
    let t = s;                       // 所有權「move」到 t，s 從此不能再用
    // println!("{}", s);            // ← 編譯錯誤！s 已經被 move 了
    println!("{}", t);               // OK
} // t 離開作用域，String 被釋放
```

> 對照 Go：所有的 `T = U` 都是 move。要做 deep copy 必須明寫 `t = s.clone()`。
> 這逼你想清楚：「這裡是要轉交所有權，還是真的要複製？」

### 2.2 Borrow（借用）—— 你會看到的 `&` 與 `&mut`

不想轉移所有權，只是「借來看一下 / 改一下」：

```rust
fn print_len(s: &String) {     // 只借不拿，函式結束後 s 還回去
    println!("{}", s.len());
}

fn append(s: &mut String) {    // 借而且要改
    s.push_str(" world");
}

fn main() {
    let mut greeting = String::from("hello");
    print_len(&greeting);       // 不可變借用：可以同時很多個
    append(&mut greeting);      // 可變借用：同一時刻只能有一個
    println!("{}", greeting);   // greeting 還活著
}
```

**借用規則**（編譯器嚴格檢查）：
- 你可以有**任意多個** `&T`（不可變借用），或者
- 你只能有**一個** `&mut T`（可變借用），
- 但**不能同時有兩種**。

這條規則直接消滅了並發程式裡 90% 的 race condition。

### 2.3 Lifetime（生命週期）

借用不能比被借的東西活得久。多數情況編譯器自動推導，你只在「函式回傳一個借用」時才需要寫：

```rust
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
```

`'a` 念作 "tick a"，是一個泛型生命週期參數，意思是「這三個引用必須活得一樣久」。
**這個專案幾乎沒寫顯式 lifetime**，因為都在編譯器能自動推導的範圍。

---

## 3. 字串：`String` vs `&str` vs `Vec<u8>` vs `&[u8]`

新手最容易卡關的地方。簡單對照表：

| 類型 | 是什麼 | 對照 |
|---|---|---|
| `String` | 擁有的、可變、UTF-8 字串 | Java `StringBuilder` |
| `&str` | 借的、不可變字串切片 | Java `String` 的 view |
| `Vec<u8>` | 擁有的、可變、任意 byte 序列 | Java `byte[]` |
| `&[u8]` | 借的、不可變 byte 切片 | Java byte[] 的 view |

**為什麼 KV 引擎用 `Vec<u8>` / `&[u8]` 不用 `String`？**
因為 KV 引擎不假設 key/value 是 UTF-8。可以存圖片、protobuf、任何二進位資料。

```rust
let key: Vec<u8> = b"hello".to_vec();   // b"..." 是 byte literal
let view: &[u8] = &key;                  // 借出整段
let prefix: &[u8] = &key[..3];           // 借出前 3 個 byte
```

---

## 4. `Option<T>` 取代 null

Rust 沒有 null。可能「沒有值」就用 `Option<T>`：

```rust
enum Option<T> {
    Some(T),
    None,
}

fn find_user(id: u64) -> Option<User> {
    if /* 找到 */ { Some(user) } else { None }
}

// 強制你處理「沒有」的情況：
match find_user(42) {
    Some(u) => println!("{}", u.name),
    None => println!("not found"),
}
```

> 對照：Java 8 的 `Optional<T>` / Go 的 `(value, ok)` / TypeScript 的 `T | undefined`。
> Rust 把這個變成語言層面的強制要求。

`Option` 上常用的方法：
- `unwrap()`：拿值；是 `None` 就 panic（測試與 demo 用）
- `unwrap_or(default)`：是 `None` 給預設
- `map(f)`：是 `Some` 就套用 f，`None` 維持 `None`
- `?`：在回傳 `Option`/`Result` 的函式裡，是 `None` 就提早回傳

---

## 5. `Result<T, E>` 取代 throw

錯誤不用 exception，用 `Result`：

```rust
enum Result<T, E> {
    Ok(T),
    Err(E),
}

fn read_file(path: &str) -> Result<String, io::Error> { ... }

// `?` 運算子：是 Err 就早退、是 Ok 就拆開
fn process(path: &str) -> Result<usize, io::Error> {
    let content = read_file(path)?;   // 自動處理 Err
    Ok(content.len())
}
```

這個專案大量用 `?`，理解它就懂 80% 的錯誤處理。

---

## 6. struct + impl：相當於 class + methods

Rust 沒有 class，但有 struct（資料）+ impl（行為），組合起來等價：

```rust
pub struct MemTable {
    map: BTreeMap<Vec<u8>, Value>,
    approximate_size: usize,
}

impl MemTable {
    pub fn new() -> Self {              // 「建構子」（其實是普通函式）
        Self { map: BTreeMap::new(), approximate_size: 0 }
    }

    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        // &mut self = 對自己的可變借用
        self.map.insert(key, Value::Put(value));
    }

    pub fn get(&self, key: &[u8]) -> Option<&Value> {
        // &self = 對自己的不可變借用
        self.map.get(key)
    }
}
```

- 第一個參數 `&self` / `&mut self` / `self` 決定「這個方法怎麼用 self」。
- 沒有 self 的就是 「associated function」，類似 Java static method。

---

## 7. enum：不是 Java enum，是「代數資料型別」

Rust 的 enum 可以**帶資料**，像 TypeScript 的 discriminated union：

```rust
pub enum Value {
    Put(Vec<u8>),    // 變體可以帶值
    Tombstone,       // 也可以不帶
}

match v {
    Value::Put(bytes) => println!("got {} bytes", bytes.len()),
    Value::Tombstone => println!("deleted"),
}
```

`Option` 與 `Result` 本身就是這種 enum。

---

## 8. trait：相當於 interface

```rust
pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
}
```

- 你可以為自己的 struct `impl Read for MyType { ... }`
- 函式可以用 `fn foo<R: Read>(r: R)` 接受任何實作了 Read 的型別
- 跟 Java `interface` / Go `interface` 概念一致，但**靜態派發**（編譯期決定，沒有 vtable 開銷）

---

## 9. cargo：Rust 的 package manager + build tool

| 指令 | 用途 |
|---|---|
| `cargo new <name>` | 建立新專案 |
| `cargo build` | 編譯（debug 模式）|
| `cargo build --release` | 編譯（最佳化）|
| `cargo run -- arg1 arg2` | 編譯並執行（`--` 後是傳給程式的參數）|
| `cargo test` | 跑全部 `#[test]` 標記的函式 |
| `cargo check` | 只做型別檢查（最快，不真的產出 binary）|
| `cargo doc --open` | 產生 HTML 文件並打開 |
| `cargo clippy` | 強烈建議的 linter |
| `cargo fmt` | 格式化 |

`Cargo.toml` 是專案描述檔（package + dependencies）。`Cargo.lock` 是鎖定的依賴版本（相當於 `package-lock.json`）。

---

## 10. 模組（module）系統

這個專案用：
- `lib.rs`：library crate 入口，宣告 `pub mod memtable;` 等
- `memtable.rs`：對應 `memtable` 模組
- `main.rs`：binary crate 入口，`use lsm_kv::LsmEngine;` 引用 library 裡的東西

`pub` 控制可見性。沒寫 pub 的東西只在自己的模組內可見。

---

## 11. 測試：寫在程式碼旁邊

```rust
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[cfg(test)]   // 只在 cargo test 時編譯
mod tests {
    use super::*;

    #[test]
    fn add_works() {
        assert_eq!(add(2, 3), 5);
    }
}
```

跑 `cargo test` 會找出所有 `#[test]` 函式並執行。
這個專案 16 個測試就是這樣寫的。

---

## 12. 你接下來該怎麼學

優先順序：

1. **The Rust Book**（官方）：<https://doc.rust-lang.org/book/>
   花 1~2 週讀前 10 章，足以開始寫實際程式。
2. **Rust by Example**：<https://doc.rust-lang.org/rust-by-example/>
   每個觀念附可執行範例，補強官方書。
3. **Rustlings**：<https://github.com/rust-lang/rustlings>
   小練習集，從零開始 fix 編譯錯誤。
4. **這個專案**：把 [docs/03-code-walkthrough.md](03-code-walkthrough.md) 當作教材，
   配著程式碼一起讀。讀懂之後挑 [docs/04-next-steps.md](04-next-steps.md) 的一個方向動手。

---

## 13. 預期的學習痛點（提前打預防針）

- **「為什麼編譯器一直罵我 borrow？」**：這是 Rust 的核心 —— 不要繞開（用 `clone()`、`Rc`、`unsafe`），
  花時間理解編譯器要你做什麼。它是對的。
- **「lifetime 寫起來很醜」**：90% 的情況不需要顯式寫，編譯器會自動推。你需要顯式寫 lifetime
  的時候，通常是程式設計可以重新組織。
- **「為什麼這麼多種字串？」**：寫多就熟。先記住「擁有用 `String/Vec`，借用用 `&str/&[u8]`」。
- **「async 怎麼這麼複雜？」**：這個專案沒用 async。等你理解 ownership 之後再學 async，會順很多。
