
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
# Role
You are a Senior Systems Architect and Rust Expert specializing in high-performance, non-standard data structures.

# Objective
Implement the `DualCache` system in Rust. 
**CRITICAL WARNING**: This is a custom topology based on "Physical Location Flow" (Viscous Array). 
- 🚫 DO NOT implement standard LRU/LFU logic. 
- 🚫 DO NOT use `LinkedHashMap` or move-to-head on access.
- ✅ Follow the specific "Swap-One" and "Evict-Point" logic described below.

# 1. Data Structures (Immutable Contract)
Use these exact struct definitions. Do not change them.

use std::sync::Arc;
use parking_lot::Mutex; // Preferred over std::sync::Mutex for performance
use arc_swap::ArcSwap;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct Node<K, V> {
    pub key: K, 
    pub value: V, 
    pub counter: u64, // Access frequency
    pub time_stamp: u64, // For expiration check
}

// The internal storage unit
struct Cache<K, V>
where
    K: Hash + Eq + Clone,
{
    arena: Vec<Node<K, V>>, // Physical rank: Index 0 is highest rank
    index: HashMap<K, usize>, // Maps Key -> Index in arena
    counter_sum: u64, 
    evict_point: usize, // The dynamic membrane index
    capacity: usize,
}

// The thread-safe wrapper
pub struct DualCache<K, V>
where
    K: Hash + Eq + Clone,
{
    main: Mutex<Cache<K, V>>, // Write Master
    mirror: ArcSwap<Cache<K, V>>, // Read Replica (Snapshot)
    lazy_update: Mutex<VecDeque<K>>, // Buffer for async updates (optional implementation)
}

# 2. Logic Specification (The Physics)

Implement the methods for `Cache` and `DualCache` following these EXACT rules:

## A. `Cache::get(key)` -> "The Viscous Climb"
1. Look up key in `index`.
2. If found at `current_idx`:
   - **Physics Rule**: Atoms struggle to move up. 
   - **Action**: If `current_idx > 0`, perform a physical `arena.swap(current_idx, current_idx - 1)`.
   - **Update**: Update `index` map for both swapped keys.
   - **Return**: Clone of the value.
   - **Constraint**: NEVER move directly to index 0. Only swap one step forward.

## B. `Cache::insert(key, value)` -> "The Gatsby Injection"
1. **Eviction**: 
   - If `arena` is full (`len == capacity`), the victim is ALWAYS the physical tail (`arena.last()`). 
   - Remove victim from `index`, overwrite `arena[last]` with new data.
   - Update `index`.
   - If not full, push to end.
2. **Placement (The Gatsby Rule)**:
   - Calculate `entry_gate = evict_point + 1`.
   - Condition: If `arena.len() > capacity / 2` AND the new item is at the tail:
     - **Action**: `arena.swap(tail_index, entry_gate)`.
     - **Meaning**: New items bypass the death zone (tail) and enter the "Probation Zone" just behind the evict_point.

## C. `Cache::update_evict_point()` -> "The Membrane Breath"
- Trigger this occasionally (e.g., during insert or get).
- **Condition**: If `arena.len() > capacity / 2`.
- **Logic**:
  - Calculate `avg = counter_sum / arena.len()`.
  - Check item at `arena[evict_point]`.
  - If `item.counter < avg`:
    - It is weak. It belongs in the Danger Zone.
    - Action: `evict_point += 1` (Expand the safe zone / Push item out).
  - Else (Strong item):
    - It holds the line. Keep `evict_point` as is (or conceptually push it slightly back, but keep logic simple).

## D. `Cache::maintenance()` -> "Time Decay"
- Iterate through all nodes in `arena`.
- Action: `node.counter >>= 1` (Bitwise right shift).
- Reset `counter_sum` based on new values.

## E. `Cache::remove(key)` -> "The Exile Protocol" (O(1) Deletion)
**CRITICAL**: DO NOT use `Vec::remove()`. That causes O(N) shifting.
Instead, implement "Swap-to-Death":

1. Look up `target_idx` in `index`.
2. **The Swap**: 
   - Swap the item at `target_idx` with the item at `arena.len() - 1` (The Physical Tail).
   - Update the `index` map for the item that was at the tail (it has now moved to `target_idx`).
3. **The Execution**:
   - Now the target item is at the tail.
   - Remove the key from `index`.
   - Perform `arena.pop()` to physically remove it from the vector.
   - *Result*: The hole is plugged by the tail element. Complexity is O(1).

## F. `Cache::cleanup_expired()` -> "The Purge"
- Iterate through arena.
- For any node where `current_time - node.timestamp > ttl`:
  - Execute **"The Exile Protocol"** (as defined above).
  - *Optimization*: Since we are iterating, we can maintain a "swap window" to keep the array compact without `pop` overhead, or just repeatedly call the O(1) remove.

# 3. DualCache Concurrency Strategy
- **Read Path (`get`)**: 
  - Try reading from `mirror` (ArcSwap) first (lock-free).
  - If hit, return. 
  - *Note*: Since `mirror` is a snapshot, strictly strictly speaking, the "Swap-One" logic implies a write. For this implementation, assume `get` acquires the `main` lock to perform the swap (or pushes to `lazy_update` queue). 
  - **Requirement**: Implement `get` by locking `main` for correctness in this version (simplest path).

# 4. Output Requirements
- Write idiomatic Rust code.
- Use `entry` API for HashMap where appropriate.
- Ensure `evict_point` stays within bounds.
- Provide comments explaining *why* a specific swap happens (e.g., "// Gatsby protection swap").
```