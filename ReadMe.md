Cache devise
K,V型態：
Arc
map結構：
資料主要儲存在hashmap可以保存檔案位置以及對應欄位
排名熱點：
每個呼叫無條件往前arena swap
累積次數：
累積呼叫次數計算平均
平均淘汰：
記憶體滿了evict point 以下的arena  truncate 每次呼叫時 確認 evict_point node counter 大約 avg是則無條件往後避免avg被扭曲
累積豁免：
有時高累積的會掉落平均值以下的arena位置則保底evict point之前
過期刷新:
log載入時間排程每天0:00檢查過期資料 根據arena 刷新hashmap 並且執行 counter >> 1
映像存取：
Blue-Green Deployment快取架構的避免hashmap鎖

#[derive(Clone, Debug)]
pub struct Node<K, V> {
    pub key: K,//檔案路徑和欄位名稱
    pub value: V,//資料
    pub counter: f64,//呼叫次數
    pub time_stamp: usize, //定期銷毀
}

struct Cache<K, V>
where
    K: Hash + Eq,
{
    arena: Vec<Node<K, V>>,//熱點排序
    index: HashMap<K, usize>,//索引
    counter_sum: f64,//呼叫總和 
    evict_point:usize,//計算呼叫平均並且truncate之後的vec 
    lazy_update:DeqVec, //main操作緩衝
}

pub trait CacheOps
{
    fn read;
    fn create; 
    fn delete; 
    fn update;
    fn daemon;
}

pub struct DualCache<K, V>
where
    K: Hash + Eq + Clone,
{
    main: Cache<K, V>,// 操作
    sub: Cache<K, V>, //映射查詢
}

 
