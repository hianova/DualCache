use std::sync::Arc;
use parking_lot::Mutex;
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::hash::Hash;
use crossbeam::channel::{Sender, Receiver, bounded};
use std::time::{SystemTime, UNIX_EPOCH};

// -----------------------------------------------------------------------------
// 1. Data Structures (Immutable Contract)
// -----------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Node<K, V> {
    pub key: K, 
    pub value: V, 
    pub counter: u64, 
    pub time_stamp: u64, 
}

#[derive(Clone)] // Derived to support Deep Clone for sync_mirror
struct Cache<K, V>
where
    K: Hash + Eq + Clone,
{
    arena: Vec<Node<K, V>>, 
    index: HashMap<K, usize>, 
    counter_sum: u64, 
    evict_point: usize, 
    capacity: usize,
}

pub struct DualCache<K, V>
where
    K: Hash + Eq + Clone,
{
    main: Mutex<Cache<K, V>>, 
    mirror: ArcSwap<Cache<K, V>>,
    lazy_tx: Sender<K>,
}

// -----------------------------------------------------------------------------
// 2. Implementation Logic
// -----------------------------------------------------------------------------

impl<K, V> DualCache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// A. Initialization
    pub fn new(capacity: usize) -> (Arc<Self>, Receiver<K>) {
        // Create bounded channel (e.g., 10,000 as suggested context)
        let (tx, rx) = bounded(10_000);

        let initial_cache = Cache {
            arena: Vec::with_capacity(capacity),
            index: HashMap::with_capacity(capacity),
            counter_sum: 0,
            evict_point: capacity, // Initialized to capacity per spec
            capacity,
        };

        let dual_cache = Arc::new(Self {
            main: Mutex::new(initial_cache.clone()),
            mirror: ArcSwap::from_pointee(initial_cache),
            lazy_tx: tx,
        });

        (dual_cache, rx)
    }

    /// B. The Read Path (Lock-Free & Lossy)
    pub fn get(&self, key: &K) -> Option<V> {
        // 1. Snapshot Access
        let cache_guard = self.mirror.load();
        
        // 2. Lazy Validation
        if let Some(&idx) = cache_guard.index.get(key) {
            // CRITICAL CHECK: Verify index bounds and key identity
            // Handles cases where index map points to truncated/reused slots
            if idx < cache_guard.arena.len() && &cache_guard.arena[idx].key == key {
                
                // 3. Lossy Signaling
                // Ignore error if full (Drop signal)
                let _ = self.lazy_tx.try_send(key.clone());

                // 4. Return value clone
                return Some(cache_guard.arena[idx].value.clone());
            }
        }

        None
    }

    /// Internal helper to sync Main state to Mirror
    fn sync_mirror(&self) {
        let main_lock = self.main.lock();
        // Deep Clone of the current main state
        let snapshot = main_lock.clone();
        // Update ArcSwap
        self.mirror.store(Arc::new(snapshot));
    }
    
    // Public wrappers for Write/Daemon operations (to be called by the Daemon thread)
    // In a real system, these would likely be called by a worker processing `rx`.
    
    pub fn process_read_signal(&self, key: K) {
        let mut guard = self.main.lock();
        guard.viscous_climb(key);
    }

    pub fn insert(&self, key: K, value: V, ttl_secs: u64) {
        let mut guard = self.main.lock();
        guard.gatsby_insert(key, value, ttl_secs);
    }

    pub fn delete(&self, key: &K) {
        let mut guard = self.main.lock();
        guard.double_swap_delete(key);
    }

    pub fn maintenance(&self) {
        let mut guard = self.main.lock();
        guard.update_evict_point();
    }
    
    pub fn update(&self, key: &K, value: V) {
        let mut guard = self.main.lock();
        guard.update_value(key, value);
    }
    
    /// Must be called manually or periodically to refresh the read-view
    pub fn commit(&self) {
        self.sync_mirror();
    }
}

// -----------------------------------------------------------------------------
// 3. Internal Cache Logic (The Write Path)
// -----------------------------------------------------------------------------

impl<K, V> Cache<K, V>
where
    K: Hash + Eq + Clone,
{
    // Helper: Gets current time as u64
    fn current_time() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    // Helper: Swaps two nodes and updates the index map
    fn swap_nodes(&mut self, idx_a: usize, idx_b: usize) {
        if idx_a == idx_b || idx_a >= self.arena.len() || idx_b >= self.arena.len() {
            return;
        }

        self.arena.swap(idx_a, idx_b);

        // Update indices for the swapped keys
        let key_a = self.arena[idx_a].key.clone();
        let key_b = self.arena[idx_b].key.clone();

        self.index.insert(key_a, idx_a);
        self.index.insert(key_b, idx_b);
    }

    /// C.1. Viscous Climb
    fn viscous_climb(&mut self, key: K) {
        // Find the key
        let current_index = match self.index.get(&key) {
            Some(&i) if i < self.arena.len() && self.arena[i].key == key => i,
            _ => return, // Key not found or invalid
        };

        // Increment counter
        self.arena[current_index].counter = self.arena[current_index].counter.saturating_add(1);
        self.counter_sum = self.counter_sum.saturating_add(1);

        // Expiration Check
        let now = Self::current_time();
        if now > self.arena[current_index].time_stamp {
            // Swap expired node with evict_point + 1
            let target = self.evict_point + 1;
            
            // Safety check: ensure target is within bounds. 
            // If arena is small, we just remove it without the specific swap logic to avoid panic.
            if target < self.arena.len() {
                self.swap_nodes(current_index, target);
            }
            
            // Remove from index (effectively validating the expiration)
            // Note: The node remains in arena (garbage) until overwritten or truncated
            self.index.remove(&key);
            return;
        }

        // Physics: Swap with current_index - 1 (Move towards 0)
        if current_index > 0 {
            self.swap_nodes(current_index, current_index - 1);
        }
    }

    /// C.2. The Gatsby Insert
    fn gatsby_insert(&mut self, key: K, value: V, ttl_secs: u64) {
        // Eviction Trigger
        if self.arena.len() == self.capacity {
            // Cliff-Edge Eviction: Truncate to evict_point
            // NOTE: Do not clean up index map here (Lazy Validation handles it)
            if self.evict_point < self.arena.len() {
                self.arena.truncate(self.evict_point);
            }
        }

        // Check if key already exists to avoid duplicates (standard cache behavior),
        // though spec focuses on "Placement". Assuming new key or overwrite via update.
        if self.index.contains_key(&key) {
            self.update_value(&key, value);
            return;
        }

        // Placement
        let time_stamp = Self::current_time() + ttl_secs;
        let node = Node {
            key: key.clone(),
            value,
            counter: 1, // Start with 1 visibility
            time_stamp,
        };
        
        // Push new node
        self.arena.push(node);
        let new_idx = self.arena.len() - 1;
        self.index.insert(key, new_idx);
        self.counter_sum = self.counter_sum.saturating_add(1);

        // Swap Rule: Immediately swap new node with node at evict_point + 1
        let target = self.evict_point + 1;
        if target < self.arena.len() {
            self.swap_nodes(new_idx, target);
        }
    }

    /// C.3. The Double-Swap Delete
    fn double_swap_delete(&mut self, key: &K) {
        let idx = match self.index.get(key) {
            Some(&i) if i < self.arena.len() && &self.arena[i].key == key => i,
            _ => return,
        };

        let target_swap_1 = self.evict_point + 1;
        
        // If the arena is too small to support the specific swap logic, just swap remove.
        if target_swap_1 >= self.arena.len() {
            // Fallback for small arenas/edge cases
            self.arena.swap_remove(idx);
            if idx < self.arena.len() {
                // swap_remove moved last to idx, update its index
                let moved_key = self.arena[idx].key.clone();
                self.index.insert(moved_key, idx);
            }
            self.index.remove(key);
            return;
        }

        // Step 1: Swap arena[idx] with arena[evict_point + 1]
        self.swap_nodes(idx, target_swap_1);

        // Step 2: Swap arena[evict_point + 1] (the target) with arena.last()
        let last_idx = self.arena.len() - 1;
        self.swap_nodes(target_swap_1, last_idx);

        // Step 3: Pop
        if let Some(node) = self.arena.pop() {
            self.index.remove(&node.key);
        }
    }

    /// C.4. Dynamic Membrane
    fn update_evict_point(&mut self) {
        if self.arena.is_empty() {
            return;
        }

        let avg = self.counter_sum / (self.arena.len() as u64).max(1);
        let step_size = (self.capacity / 10).max(1);

        // Check if average suggests expansion (simple heuristic based on activity)
        // If the global sum is high relative to length, traffic is high, widen the safe zone.
        // (Logic inferred from "Counter sum suggests avg has increased")
        // Note: Real implementation might track previous avg to detect increase.
        // Here we assume high average score implies we need more space protected.
        
        // Expansion logic: If evict point is small but avg is high, move evict_point forward (larger index)
        if self.evict_point < self.capacity {
             // Heuristic: If we are truncating too aggressively but nodes are hot
            self.evict_point = (self.evict_point + step_size).min(self.capacity);
        }

        // Contraction: If the node AT evict_point is Strong (counter > avg)
        // It "holds the line", effectively pushing the membrane back (or resisting move).
        // Spec: "If node at evict_point has counter > avg... it holds the line."
        // Interpreted as: If the border node is strong, we don't truncate it easily, 
        // so we might actually reduce evict_point to tighten the circle or keep it there.
        // HOWEVER, context implies "Membrane" moves to optimize cache.
        // Let's implement Contraction as reducing `evict_point` if the boundary is weak?
        // No, prompt says: "If node ... > avg (Strong Node), it holds the line."
        // This usually implies preventing the evict_point from moving past it (shrinking the safe zone).
        
        // Let's implement a specific check:
        if self.evict_point < self.arena.len() {
            let boundary_node = &self.arena[self.evict_point];
            if boundary_node.counter > avg {
                // Strong node at border. 
                // We do NOT contract (reduce index). We leave it or expand.
            } else {
                // Weak node at border. The membrane contracts (moves toward 0),
                // making the "safe zone" smaller and "at risk" zone larger.
                self.evict_point = self.evict_point.saturating_sub(step_size);
            }
        }
        
        // Safety: Ensure evict_point stays within bounds relative to capacity
        if self.evict_point > self.capacity {
            self.evict_point = self.capacity;
        }
    }

    /// C.5. Updates
    fn update_value(&mut self, key: &K, value: V) {
         if let Some(&idx) = self.index.get(key) {
             if idx < self.arena.len() && &self.arena[idx].key == key {
                 self.arena[idx].value = value;
                 // Constraint: Do NOT reset counter or rank (index).
                 // Done.
             }
         }
    }
}
//code support by gemini 3.0