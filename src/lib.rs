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
        Cache {
            arena: Vec::with_capacity(capacity),
            index: HashMap::new(),
            counter_sum: 0,
            evict_point: 0,
            capacity,
        }
    }

    /// 黏性爬升：元素只能與前一個位置交換，模擬在黏性介質中的緩慢上浮
    pub fn get(&mut self, key: &K) -> Option<V> {
        let current_idx = *self.index.get(key)?;
        
        // 增加存取計數，模擬「浮力增加」
        self.arena[current_idx].counter += 1;
        self.counter_sum += 1;
        
        // 物理規則：只能與相鄰的上一個位置交換
        // 這防止了單次存取就造成的位置劇變
        if current_idx > 0 {
            let swap_target = current_idx - 1;
            
            // 執行物理交換
            self.arena.swap(current_idx, swap_target);
            
            // 同步更新索引映射
            let key_at_target = self.arena[current_idx].key.clone();
            let accessed_key = self.arena[swap_target].key.clone();
            
            self.index.insert(accessed_key, swap_target);
            self.index.insert(key_at_target, current_idx);
        }
        
        // 定期觸發膜點呼吸
        if self.arena.len() > 2 && self.counter_sum % 10 == 0 {
            self.update_evict_point();
        }
        
        Some(self.arena[self.index[key]].value.clone())
    }

    /// 蓋茲比注入：新元素繞過死亡區，直接進入試用區
    pub fn insert(&mut self, key: K, value: V, time_stamp: u64) {
        // 檢查是否為更新操作
        if let Some(&existing_idx) = self.index.get(&key) {
            self.arena[existing_idx].value = value;
            self.arena[existing_idx].counter += 1;
            self.arena[existing_idx].time_stamp = time_stamp;
            self.counter_sum += 1;
            return;
        }

        let new_node = Node {
            key: key.clone(),
            value,
            counter: 1,
            time_stamp,
        };

        // 容量管理：物理尾端是犧牲品
        if self.arena.len() >= self.capacity {
            let victim_idx = self.arena.len() - 1;
            let victim_key = self.arena[victim_idx].key.clone();
            
            // 移除犧牲品的索引記錄
            self.counter_sum = self.counter_sum.saturating_sub(self.arena[victim_idx].counter);
            self.index.remove(&victim_key);
            
            // 直接覆寫尾端位置
            self.arena[victim_idx] = new_node;
            self.index.insert(key.clone(), victim_idx);
            
            // 蓋茲比保護交換：如果快取已半滿且 evict_point 有效
            if self.arena.len() > self.capacity / 2 && self.evict_point < victim_idx {
                let entry_gate = (self.evict_point + 1).min(victim_idx);
                
                // 新元素跳過死亡區，進入試用區
                self.arena.swap(victim_idx, entry_gate);
                
                // 同步索引更新
                let key_at_gate = self.arena[victim_idx].key.clone();
                self.index.insert(key.clone(), entry_gate);
                self.index.insert(key_at_gate, victim_idx);
            }
        } else {
            // 空間充足時直接推入尾端
            let insert_idx = self.arena.len();
            self.arena.push(new_node);
            self.index.insert(key, insert_idx);
        }
        
        self.counter_sum += 1;
        
        // 插入後重新評估膜點位置
        if self.arena.len() > self.capacity / 2 {
            self.update_evict_point();
        }
    }

    /// 膜點呼吸：根據元素強度動態調整安全區邊界
    fn update_evict_point(&mut self) {
        if self.arena.is_empty() || self.arena.len() <= self.capacity / 2 {
            return;
        }

        // 計算平均存取強度作為「存活門檻」
        let avg_counter = self.counter_sum / self.arena.len() as u64;
        
        // 確保 evict_point 在有效範圍內
        if self.evict_point >= self.arena.len() {
            self.evict_point = (self.arena.len() / 2).max(1) - 1;
        }

        let item_at_membrane = &self.arena[self.evict_point];
        
        if item_at_membrane.counter < avg_counter {
            // 該元素弱於平均值，應該被推向危險區
            // 膜點向後移動，擴張安全區（或說收縮該元素的保護範圍）
            self.evict_point = (self.evict_point + 1).min(self.arena.len() - 1);
        } else {
            // 該元素足夠強壯，守住防線
            // 可以選擇性地收縮 evict_point，但為了穩定性暫時保持不變
            // 若要實作「回縮」邏輯，可以在此處 evict_point -= 1
        }
    }

    /// 時間衰減：所有計數器右移一位，模擬記憶淡化
    pub fn maintenance(&mut self) {
        self.counter_sum = 0;
        
        for node in &mut self.arena {
            node.counter >>= 1;
            self.counter_sum += node.counter;
        }
    }

    /// 過期清理：移除時間戳過舊的節點
    pub fn cleanup_expired(&mut self, current_time: u64, ttl: u64) {
        let expired_keys: Vec<K> = self.arena
            .iter()
            .filter(|node| current_time.saturating_sub(node.time_stamp) > ttl)
            .map(|node| node.key.clone())
            .collect();

        for key in expired_keys {
            self.remove(&key);
        }
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let idx = self.index.remove(key)?;
        let removed_node = self.arena.remove(idx);
        
        self.counter_sum = self.counter_sum.saturating_sub(removed_node.counter);
        
        // 更新所有後續元素的索引
        for i in idx..self.arena.len() {
            let k = self.arena[i].key.clone();
            self.index.insert(k, i);
        }
        
        // 調整 evict_point 避免越界
        if self.evict_point >= self.arena.len() && self.evict_point > 0 {
            self.evict_point = self.arena.len().saturating_sub(1);
        }
        
        Some(removed_node.value)
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
    K: Hash + Eq + Clone + Send + Sync,
    V: Clone + Send + Sync,
{
    pub fn new(capacity: usize) -> Self {
        let cache = Cache::new(capacity);
        let mirror_cache = Cache::new(capacity);
        
        DualCache {
            main: Mutex::new(cache),
            mirror: ArcSwap::new(Arc::new(mirror_cache)),
            lazy_update: Mutex::new(VecDeque::new()),
        }
    }

    /// 讀取路徑：優先鎖定主快取執行黏性爬升
    /// 注意：由於 swap 操作本質是寫入，這裡必須獲取寫鎖
    pub fn get(&self, key: &K) -> Option<V> {
        let mut main = self.main.lock();
        main.get(key)
    }

    /// 寫入路徑：執行蓋茲比注入
    pub fn insert(&self, key: K, value: V, time_stamp: u64) {
        let mut main = self.main.lock();
        main.insert(key, value, time_stamp);
    }

    /// 定期同步：將主快取快照複製到鏡像（若實作讀優化）
    pub fn sync_mirror(&self) {
        let main = self.main.lock();
        
        // 創建主快取的深拷貝
        let snapshot = Cache {
            arena: main.arena.clone(),
            index: main.index.clone(),
            counter_sum: main.counter_sum,
            evict_point: main.evict_point,
            capacity: main.capacity,
        };
        
        self.mirror.store(Arc::new(snapshot));
    }

    /// 維護操作：時間衰減與過期清理
    pub fn maintenance(&self, current_time: u64, ttl: u64) {
        let mut main = self.main.lock();
        main.maintenance();
        main.cleanup_expired(current_time, ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viscous_climb() {
        let cache = DualCache::new(5);
        
        // 插入五個元素
        for i in 0..5 {
            cache.insert(i, format!("value_{}", i), 100);
        }
        
        // 多次存取索引 4 的元素
        // 它應該逐步向上爬升，而非直接跳到頂端
        for _ in 0..3 {
            cache.get(&4);
        }
        
        // 驗證黏性爬升行為
        let main = cache.main.lock();
        let pos = main.index[&4];
        assert!(pos < 4, "元素應該上升但不是瞬間到頂");
    }

    #[test]
    fn test_gatsby_injection() {
        let cache = DualCache::new(4);
        
        // 填滿快取
        for i in 0..4 {
            cache.insert(i, i * 10, 100);
        }
        
        // 插入新元素，觸發蓋茲比規則
        cache.insert(99, 990, 100);
        
        let main = cache.main.lock();
        
        // 新元素不應該在物理尾端（死亡區）
        let pos = main.index[&99];
        assert!(pos < main.arena.len() - 1, "新元素應繞過死亡區");
    }
}

//code support by claude sonnet4.5