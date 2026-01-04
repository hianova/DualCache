
# DualCache

> **Status: Experimental / Proof of Concept**
>
> *A high-performance, concurrency-friendly caching architecture designed for read-heavy, power-law distributed workloads.*

## 📖 Introduction

**DualCache** is a Rust-based caching library that challenges the traditional LRU/LFU implementations. Instead of relying on complex lock-free linked lists or micro-managed atomic memory orderings, DualCache leverages a **Blue-Green Deployment Architecture** (Double Buffering) to separate reads from writes completely.

This design eliminates reader lock contention and optimizes for **CPU Cache Locality** by using contiguous memory layouts (`Vec`) instead of pointer chasing. It is specifically engineered for systems where **throughput** and **tail latency stability** are critical, such as high-frequency trading systems, blockchain state storage, and high-traffic web services.

## 🚀 Key Features

### 1. Blue-Green Architecture (Read-Write Splitting)
*   **Zero-Contention Reads:** Utilizes a `Main` (Writer) and `Sub` (Reader) structure. Readers access a "Snapshot" of the data, ensuring `O(1)` wait-free access without being blocked by ongoing writes or evictions.
*   **Lazy Consistency:** Updates are batched and synchronized based on a "materiality" threshold, prioritizing system throughput over immediate strong consistency.

### 2. Statistical Eviction Protocol
*   **Mean-Based Threshold:** Instead of a rigid LRU queue, eviction is determined by dynamic statistical analysis (Global Counter Sum / Count). This effectively handles **Power-Law (Zipfian) Distributions** where "legacy authorities" (historically hot items) should not be evicted due to temporary inactivity.
*   **Legacy Protection:** A "Grandfather Clause" mechanism prevents high-value data from being flushed out by short-term traffic spikes (Scan Resistance).

### 3. Hardware-Aware Optimization
*   **Vec > Linked List:** All data resides in contiguous `Vec` structures. Reordering is done via `swap` or memory rotation, maximizing **L1/L2 Cache Hits** and avoiding the expensive pointer chasing found in traditional cache implementations.
*   **Simplicity by Design:** Intentionally avoids complex `Relaxed`/`Acquire` atomic orderings in favor of a macro-architectural design that eliminates the *need* for fine-grained synchronization.

### 4. Batching & Compression (Log Compaction)
*   **DeqVec Queue:** Write operations and promotion requests are buffered in a queue.
*   **Noise Filtering:** The system employs a "Log Compaction" strategy to merge redundant updates (e.g., +1, +1, +1 → +3) before applying them, significantly reducing write amplification.

## 🛠️ Architecture Overview

```rust
pub struct DualCache<K, V> {
    // The "Writer" - Handles mutations, evictions, and heavy lifting.
    main: Cache<K, V>, 
    
    // The "Reader" - A lightweight, read-only snapshot for high-throughput access.
    sub: Cache<K, V>,  
    
    // Asynchronous control plane for handling batched updates.
    lazy_update: DeqVec, 
}
```

### The "Sweet Spot" Philosophy
DualCache is built on the belief that **Architecture > Micro-optimization**. By isolating readers from writers and using statistical averages for eviction, we achieve a system that is:
1.  **Robust:** Resistant to cache thrashing.
2.  **Predictable:** Flat latency curves with minimal jitter.
3.  **Maintainable:** Simple, reasoning-friendly code without `unsafe` spaghetti.

## 📦 Usage

*(Note: The API is subject to change as this is a Proof of Concept)*

```rust
use dual_cache::DualCache;
use std::sync::Arc;

fn main() {
    // Initialize DualCache
    let cache = DualCache::new();

    // Insert data (Goes to Main, eventually synced to Sub)
    cache.insert("user_123", "SessionData");

    // High-concurrency read (Hits Sub, wait-free)
    if let Some(value) = cache.get("user_123") {
        println!("Found: {}", value);
    }
    
    // The daemon/scheduler handles eviction and sync in the background
    // based on statistical analysis of traffic patterns.
}
```

## 🔮 Roadmap

*   [ ] **Micro-Benchmarks:** Comparative analysis against `moka`, `dashmap`, and standard `RwLock`.
*   [ ] **Fuzz Testing:** Using `loom` to verify concurrency safety under extreme chaos.
*   [ ] **Adaptive Thresholds:** Implementing linear regression to predict traffic gaps for optimal sync timing.

## 📄 License

[PolyForm Noncommercial License 1.0.0](https://polyformproject.org/licenses/noncommercial/1.0.0/)
---
**Disclaimer:** This project is an architectural study in high-performance system design. While the logic is sound, it is currently in an experimental phase. Contributions and discussions are welcome.

## 🤖AI generate code promt

```
DualCache 設計底稿
-K,V型態：
Arc（Zero-copy cloning）
-map結構：
資料主要儲存在arena可以保存檔案位置以及對應欄位
-排名熱點：
每個呼叫無條件往前arena swap
-累積次數：
累積呼叫次數計算平均
-平均淘汰：
到達arena capacity evict_point 以下  truncate 
-累積豁免：
有時高累積的會掉落平均值以下的arena位置則保底evict_point之前
-過期刷新:
排程每天0:00檢查time_stamp 根據arena 刷新hashmap index 並且執行 counter >> 1
-映像存取：
Blue-Green Deployment快取架構的避免hashmap鎖
-新增豁免：
arena.len> capacity/2 時 新增資料append後evict_point+1 與其互換位置
-evict_point刷新：
arena.len> capacity/2 時 每次呼叫其他資料檢查evict_point counter 是否小於avg 否則evict_point +1


#[derive(Clone, Debug)]
pub struct Node<K, V> {
    pub key: K, //檔案路徑和欄位名稱
    pub value: V, //資料
    pub counter: u64, //呼叫次數
    pub time_stamp: u64, //定期銷毀
}

struct Cache<K, V>
where
    K: Hash + Eq,
{
    arena: Vec<Node<K, V>>, //熱點儲存排序 
    index: HashMap<K, usize>, //索引 
    counter_sum: usize, //呼叫總和 預設：0
    evict_point:usize, //平均對應節點 預設：capacity/2
    capacity:usize, //自定義容器
}

pub struct DualCache<K, V>
where
    K: Hash + Eq + Clone,
{
    main: Mutex<Cache<K, V>>,// 操作
    mirror: Arcswap<Cache<K, V>>, //映射查詢
    lazy_update:Mutex<VecDeque<CacheAction<K>>>, //main操作緩衝
}

impl DualCache{
    fn daemon;
}
```

 
