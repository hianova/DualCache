use std::sync::Arc;
use parking_lot::Mutex;
use arc_swap::ArcSwap;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

#[derive(Clone, Debug)]
pub struct Node<K, V> {
    pub key: K,
    pub value: V,
    pub counter: u64,
    pub time_stamp: u64,
}

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

impl<K, V> Cache<K, V>
where
    K: Hash + Eq + Clone,
    V: Clone,
{
    fn new(capacity: usize) -> Self {
        Self {
            arena: Vec::with_capacity(capacity),
            index: HashMap::with_capacity(capacity),
            counter_sum: 0,
            evict_point: 0,
            capacity,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let current_idx = *self.index.get(key)?;
        
        // 增加存取計數器
        self.arena[current_idx].counter += 1;
        self.counter_sum += 1;
        
        // 黏性向上爬升：僅與前一個位置交換，不直接跳到頂端
        if current_idx > 0 {
            let prev_idx = current_idx - 1;
            
            // 執行物理交換
            self.arena.swap(current_idx, prev_idx);
            
            // 更新索引映射：兩個被交換的鍵都需要更新位置
            let swapped_key = self.arena[current_idx].key.clone();
            self.index.insert(key.clone(), prev_idx);
            self.index.insert(swapped_key, current_idx);
        }
        
        Some(self.arena[self.index[key]].value.clone())
    }

    fn insert(&mut self, key: K, value: V, timestamp: u64) {
        // 檢查是否已存在該鍵
        if let Some(&existing_idx) = self.index.get(&key) {
            self.arena[existing_idx].value = value;
            self.arena[existing_idx].counter += 1;
            self.arena[existing_idx].time_stamp = timestamp;
            self.counter_sum += 1;
            return;
        }

        let new_node = Node {
            key: key.clone(),
            value,
            counter: 1,
            time_stamp: timestamp,
        };
        self.counter_sum += 1;

        // 驅逐邏輯：滿載時替換尾端受害者
        if self.arena.len() == self.capacity {
            let victim_key = self.arena.last().unwrap().key.clone();
            self.index.remove(&victim_key);
            self.counter_sum -= self.arena.last().unwrap().counter;
            
            let last_idx = self.arena.len() - 1;
            self.arena[last_idx] = new_node;
            self.index.insert(key.clone(), last_idx);
        } else {
            // 未滿時推入尾端
            let new_idx = self.arena.len();
            self.arena.push(new_node);
            self.index.insert(key.clone(), new_idx);
        }

        // 蓋茲比規則：新進入者獲得保護，繞過死亡區
        if self.arena.len() > self.capacity / 2 {
            let tail_idx = self.arena.len() - 1;
            let entry_gate = (self.evict_point + 1).min(tail_idx);
            
            if entry_gate < tail_idx {
                // 執行蓋茲比保護交換
                self.arena.swap(tail_idx, entry_gate);
                
                let gate_key = self.arena[tail_idx].key.clone();
                self.index.insert(key.clone(), entry_gate);
                self.index.insert(gate_key, tail_idx);
            }
        }

        self.update_evict_point();
    }

    fn update_evict_point(&mut self) {
        if self.arena.len() <= self.capacity / 2 {
            return;
        }

        if self.arena.is_empty() {
            return;
        }

        let avg = self.counter_sum / self.arena.len() as u64;
        
        // 確保驅逐點不超出範圍
        if self.evict_point >= self.arena.len() {
            self.evict_point = self.arena.len().saturating_sub(1);
        }

        // 膜呼吸：檢查驅逐點處的項目強度
        if self.evict_point < self.arena.len() {
            let item_counter = self.arena[self.evict_point].counter;
            
            if item_counter < avg {
                // 弱項目：擴展安全區，將其推入危險區
                self.evict_point = (self.evict_point + 1).min(self.arena.len().saturating_sub(1));
            }
            // 強項目：維持現狀，守住防線
        }
    }

    fn maintenance(&mut self) {
        self.counter_sum = 0;
        
        // 時間衰減：所有計數器右移一位
        for node in &mut self.arena {
            node.counter >>= 1;
            self.counter_sum += node.counter;
        }
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let target_idx = *self.index.get(key)?;
        
        // 流放協議：交換至死亡位置實現 O(1) 刪除
        let tail_idx = self.arena.len() - 1;
        
        if target_idx != tail_idx {
            // 執行交換
            self.arena.swap(target_idx, tail_idx);
            
            // 更新被交換到目標位置的項目索引
            let moved_key = self.arena[target_idx].key.clone();
            self.index.insert(moved_key, target_idx);
        }
        
        // 執行處決
        self.index.remove(key);
        let removed = self.arena.pop()?;
        self.counter_sum -= removed.counter;
        
        // 調整驅逐點以防止越界
        if self.evict_point >= self.arena.len() && self.evict_point > 0 {
            self.evict_point = self.arena.len().saturating_sub(1);
        }
        
        Some(removed.value)
    }

    fn cleanup_expired(&mut self, current_time: u64, ttl: u64) {
        let mut i = 0;
        while i < self.arena.len() {
            if current_time.saturating_sub(self.arena[i].time_stamp) > ttl {
                let key = self.arena[i].key.clone();
                self.remove(&key);
                // 不增加 i，因為刪除後當前位置已是新項目
            } else {
                i += 1;
            }
        }
    }
}

pub struct DualCache<K, V>
where
    K: Hash + Eq + Clone,
{
    main: Mutex<Cache<K, V>>,
    mirror: ArcSwap<Cache<K, V>>,
    lazy_update: Mutex<VecDeque<K>>,
}

impl<K, V> DualCache<K, V>
where
    K: Hash + Eq + Clone,
    V: Clone,
{
    pub fn new(capacity: usize) -> Self {
        let cache = Cache::new(capacity);
        Self {
            main: Mutex::new(Cache::new(capacity)),
            mirror: ArcSwap::new(Arc::new(cache)),
            lazy_update: Mutex::new(VecDeque::new()),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        // 為正確性起見，直接鎖定主快取執行黏性交換
        let mut main = self.main.lock();
        main.get(key)
    }

    pub fn insert(&self, key: K, value: V, timestamp: u64) {
        let mut main = self.main.lock();
        main.insert(key, value, timestamp);
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        let mut main = self.main.lock();
        main.remove(key)
    }

    pub fn maintenance(&self) {
        let mut main = self.main.lock();
        main.maintenance();
    }

    pub fn cleanup_expired(&self, current_time: u64, ttl: u64) {
        let mut main = self.main.lock();
        main.cleanup_expired(current_time, ttl);
    }

    pub fn sync_mirror(&self) {
        let main = self.main.lock();
        let snapshot = Cache {
            arena: main.arena.clone(),
            index: main.index.clone(),
            counter_sum: main.counter_sum,
            evict_point: main.evict_point,
            capacity: main.capacity,
        };
        self.mirror.store(Arc::new(snapshot));
    }
}
// code support by claude sonnet4.5