
# DualCache

**DualCache** is a high-performance, thread-safe caching system in Rust designed for extreme read concurrency. It prioritizes **latency stability** and **lock-free reads** over strict accounting precision, utilizing a statistical approach to ranking and eviction.

Unlike traditional LRU/LFU caches that rely on heavy locking or complex linked lists, DualCache employs a **"Viscous Array"** topology with a **Read-Write Separation** architecture.

## 🚀 Key Features

*   **Lock-Free Read Path**: Readers access a read-only snapshot (`mirror`) via `ArcSwap`. No Mutex contention on the read path.
*   **Viscous Climb Ranking**: Hot items physically bubble up the array (`index` swaps with `index - 1`) based on access, mimicking fluid dynamics.
*   **Lossy Signaling (Backpressure)**: Access counters are updated via a bounded async channel. If the channel is full, the signal is **dropped**. This guarantees that ranking logic never blocks the reader.
*   **Cliff-Edge Eviction**: Eviction is performed via `Vec::truncate` from a dynamic `evict_point`, instantly freeing capacity without iterating through linked lists.
*   **Lazy Validation**: Handles dangling indices (caused by async truncation) via O(1) boundary and key checks during reads.
*   **Swap-to-Delete**: Deletions are O(1) by swapping the target with the physical tail and popping, preserving memory density.

## 🏗 Architecture

DualCache splits the world into two dimensions to solve the "Read-as-Write" lock contention problem:

1.  **The Mirror (Read-Path)**: An `ArcSwap<Cache>` snapshot. Readers access this lock-free.
2.  **The Main (Write-Path)**: A `Mutex<Cache>` protected master copy.
3.  **The Signal Channel**: A bounded MPSC channel (`Sender<K>`). Readers throw keys into this channel to signal "hits".
4.  **The Daemon**: A background worker that drains the channel, updates the `Main` structure (ranking/counters), and periodically updates the `Mirror`.

## ⚙️ Core Mechanisms

### 1. The Viscous Climb (Read Promotion)
When a key is accessed:
1.  The reader sends the key to the Daemon (Fire-and-forget).
2.  The Daemon increments the counter.
3.  **Physics**: The item swaps places with the item at `index - 1`.
    *   *Result*: Hot items naturally rise to the top. Cold items are physically pushed down by the rising hot items.

### 2. The Gatsby Injection (Insertion)
New items are not placed at the top. They are swapped into the **"Probation Zone"** (just after `evict_point + 1`). They must earn their way to the top via reads; otherwise, they are prime candidates for the next eviction.

### 3. Cliff-Edge Eviction
Instead of removing items one by one:
1.  A dynamic `evict_point` is calculated based on the average hit count (`counter_sum / len`).
2.  Items below the average are candidates for eviction.
3.  When capacity is full, the underlying vector is **truncated** at `evict_point`.
    *   *Note*: This may leave "dangling indices" in the HashMap, which are lazily cleaned up during the next read attempt.

### 4. Lossy Statistics
We accept that under extreme load (e.g., DDoS), accurate counting is impossible without blocking.
*   **Policy**: If the update channel is full, **drop the packet**.
*   **Theory**: Statistical Law of Large Numbers. High-frequency keys will still statistically dominate the ranking even with 5-10% signal loss. Latency consistency is preferred over perfect accounting.

## 📦 Installation & Usage

Add `crossbeam-channel`, `parking_lot`, and `arc-swap` to your `Cargo.toml`.

```rust
use std::sync::Arc;
use std::thread;
use dual_cache::DualCache; // Assuming crate name

fn main() {
    // 1. Initialize DualCache with capacity 1,000,000
    // Returns the Cache instance (Arc) and the Receiver for the Daemon
    let (cache, rx) = DualCache::new(1_000_000);

    // 2. Spawn the Daemon (The Maintenance Worker)
    let cache_for_daemon = cache.clone();
    thread::spawn(move || {
        // The Daemon drains the queue and performs physical mutations
        while let Ok(key) = rx.recv() {
            // Internal logic: 
            // - Locks Main
            // - Updates Counter / Performs Viscous Climb
            // - Updates Mirror Snapshot occasionally
            cache_for_daemon.handle_update(key); 
        }
    });

    // 3. High-Concurrency Reads (Lock-Free)
    let cache_ref = cache.clone();
    thread::spawn(move || {
        if let Some(value) = cache_ref.get(&"my_key") {
            println!("Got value: {:?}", value);
        }
    });
}
```

## 🧩 Data Structures

```rust
pub struct DualCache<K, V> {
    main: Mutex<Cache<K, V>>,       // Write Master
    mirror: ArcSwap<Cache<K, V>>,   // Read Replica
    lazy_tx: Sender<K>,             // Async Signal Channel
}
```

## ⚖️ Performance Philosophy

*   **P99 Stability**: By decoupling the accounting logic from the read path, `Read` operations are purely memory lookups + a non-blocking channel send. Even if the Daemon stalls, readers continue to serve data at microsecond speeds.
*   **Self-Healing**: "Zombie" data (data swapped into high ranks due to deletion logic) is naturally purged. If it is cold, real hot data will "climb" over it, pushing the zombie down to the eviction zone automatically.

## License

