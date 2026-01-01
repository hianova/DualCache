use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Node<K, V> {
    pub key: K,
    pub value: V, // V 通常是 Arc<Data>
    pub counter: f64, // 使用 f64 支援更平滑的衰減計算
    pub time_stamp: usize, 
}

// 用於 lazy_update 的操作指令
pub enum CacheAction<K> {
    Hit(K),
    Create(K),
    Delete(K),
}

struct Cache<K, V>
where
    K: Hash + Eq + Clone,
{
    arena: Vec<Node<K, V>>,
    index: HashMap<K, usize>,
    counter_sum: f64,
    evict_point: usize,
    capacity: usize,
}
pub trait CacheOps<K, V> {
    fn read(&self, key: &K) -> Option<V>;
    fn create(&mut self, key: K, value: V);
    fn delete(&mut self, key: &K);
    fn update(&mut self, key: K, value: V);
    fn daemon(&mut self); // 治理邏輯
}

pub struct DualCache<K, V>
where
    K: Hash + Eq + Clone,
{
    // Production: 負責 Read (Blue)
    pub sub: Arc<Cache<K, V>>, 
    // Governance: 負責 Write & Sort (Green)
    main: Cache<K, V>,
    // 情報層：限制訊息廣播範圍的管道
    lazy_update: VecDeque<CacheAction<K>>,
}

impl<K, V> CacheOps<K, V> for DualCache<K, V>
where
    K: Hash + Eq + Clone,
    V: Clone,
{
    fn read(&self, key: &K) -> Option<V> {
        // 1. 直接存取 sub，不加鎖，不更新排序
        self.sub.index.get(key).map(|&idx| self.sub.arena[idx].value.clone())
        // 註：Hit 的訊息會透過外部呼叫或異步傳入 lazy_update，不影響 read 速度
    }

    fn create(&mut self, key: K, value: V) {
        self.lazy_update.push_back(CacheAction::Create(key));
        // 實際插入發生在 main，sub 等待下一輪 flip
    }

    fn delete(&mut self, key: &K) {
        self.lazy_update.push_back(CacheAction::Delete(key.clone()));
    }

    fn update(&mut self, key: K, value: V) {
        // 同 Create 邏輯，在 governance 層處理
    }

    fn daemon(&mut self) {
        // 這是三角形頂端的「決策層」
        // 1. 消耗 lazy_update，執行 arena swap
        self.process_updates();
        
        // 2. 豁免權檢查與排隊
        self.apply_exemption_logic();

        // 3. 檢查容量並執行 Truncate
        if self.main.arena.len() > self.main.capacity {
            self.evict();
        }

        // 4. Blue-Green Flip: 將治理完的結果同步到 sub
        // 因為 Node 內是 Arc，這裡的 clone 非常輕量
        self.sub = Arc::new(self.main.clone());
    }
}
impl<K, V> DualCache<K, V> where K: Hash + Eq + Clone {
    
    fn process_updates(&mut self) {
        while let Some(action) = self.lazy_update.pop_front() {
            match action {
                CacheAction::Hit(k) => {
                    if let Some(&idx) = self.main.index.get(&k) {
                        self.main.arena[idx].counter += 1.0;
                        self.main.counter_sum += 1.0;
                        
                        // 無條件往前 swap: 階級流動
                        if idx > 0 {
                            let prev = idx - 1;
                            self.main.arena.swap(idx, prev);
                            // 更新索引
                            self.main.index.insert(self.main.arena[idx].key.clone(), idx);
                            self.main.index.insert(self.main.arena[prev].key.clone(), prev);
                        }
                    }
                }
                // Handle Create/Delete...
                _ => {}
            }
        }
    }

    fn apply_exemption_logic(&mut self) {
        let avg = self.main.counter_sum / self.main.arena.len() as f64;
        let ep = self.main.evict_point;

        if ep < self.main.arena.len() {
            let ep_node_counter = self.main.arena[ep].counter;

            // 累積豁免：高於平均則保證在 ep 之前
            if ep_node_counter > avg {
                // 找一個 ep 之前但 counter 低於平均的「平庸者」交換
                for i in 0..ep {
                    if self.main.arena[i].counter < avg {
                        self.main.arena.swap(i, ep);
                        // 同步更新 HashMap 索引 (省略程式碼)
                        break;
                    }
                }
            } 
            // 避免 avg 扭曲：如果 ep 上的資料太爛，無條件往後丟
            else if ep_node_counter < (avg * 0.1) {
                let tail = self.main.arena.len() - 1;
                self.main.arena.swap(ep, tail);
            }
        }
    }

    fn evict(&mut self) {
        // 同步 HashMap：在截斷前移除被拋棄的 Key
        for i in self.main.evict_point..self.main.arena.len() {
            let node = &self.main.arena[i];
            self.main.index.remove(&node.key);
            self.main.counter_sum -= node.counter;
        }
        // 極速截斷
        self.main.arena.truncate(self.main.evict_point);
    }

    pub fn midnight_refresh(&mut self) {
        // 0:00 執行：老化機制與索引重構
        for node in &mut self.main.arena {
            node.counter /= 2.0; // counter >> 1 的 f64 版本
        }
        self.main.counter_sum /= 2.0;
        
        // 根據 Arena 重刷 HashMap，確保 100% 正確
        self.main.index.clear();
        for (idx, node) in self.main.arena.iter().enumerate() {
            self.main.index.insert(node.key.clone(), idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
