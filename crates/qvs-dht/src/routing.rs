use std::collections::VecDeque;
use std::time::{Duration, Instant};

use qvs_core::{KBucketEntry, NodeId};

pub const K: usize = 8;
const REFRESH_INTERVAL: Duration = Duration::from_secs(900);
const STALE_INTERVAL: Duration = Duration::from_secs(900);

#[derive(Debug, Clone)]
pub struct KBucket {
    entries: VecDeque<KBucketEntry>,
    last_refreshed: Instant,
}

impl KBucket {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(K),
            last_refreshed: Instant::now(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.entries.len() >= K
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn insert(&mut self, entry: KBucketEntry) -> InsertResult {
        if let Some(pos) = self.entries.iter().position(|e| e.node_id == entry.node_id) {
            self.entries[pos].last_seen = Instant::now();
            self.entries[pos].latency = entry.latency;
            self.entries.rotate_left(pos);
            return InsertResult::Existing;
        }

        if !self.is_full() {
            self.entries.push_back(entry);
            return InsertResult::Added;
        }

        if let Some(stale_pos) = self
            .entries
            .iter()
            .position(|e| e.last_seen.elapsed() > STALE_INTERVAL)
        {
            self.entries.remove(stale_pos);
            self.entries.push_back(entry);
            return InsertResult::Replaced;
        }

        InsertResult::Full
    }

    pub fn remove(&mut self, node_id: &NodeId) {
        self.entries.retain(|e| e.node_id != *node_id);
    }

    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<KBucketEntry> {
        let mut candidates: Vec<&KBucketEntry> = self.entries.iter().collect();
        candidates.sort_by(|a, b| {
            let da = a.node_id.xor_distance(target);
            let db = b.node_id.xor_distance(target);
            da.cmp(&db)
        });
        candidates.into_iter().take(count).cloned().collect()
    }

    pub fn needs_refresh(&self) -> bool {
        self.last_refreshed.elapsed() > REFRESH_INTERVAL
    }

    pub fn mark_refreshed(&mut self) {
        self.last_refreshed = Instant::now();
    }

    pub fn entries(&self) -> &VecDeque<KBucketEntry> {
        &self.entries
    }

    pub fn get_last_refreshed(&self) -> Instant {
        self.last_refreshed
    }

    pub fn split(&mut self, local_id: &NodeId, bucket_idx: usize) -> (Self, Self) {
        let mut left = Self::new();
        let mut right = Self::new();
        let split_bit = bucket_idx;
        let byte_idx = split_bit / 8;
        let bit_mask = 1 << (7 - (split_bit % 8));
        while let Some(entry) = self.entries.pop_front() {
            let dist = local_id.xor_distance(&entry.node_id);
            if (dist[byte_idx] & bit_mask) == 0 {
                left.entries.push_back(entry);
            } else {
                right.entries.push_back(entry);
            }
        }
        (left, right)
    }
}

impl Default for KBucket {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertResult {
    Added,
    Existing,
    Replaced,
    Full,
}

pub struct RoutingTable {
    local_id: NodeId,
    buckets: Vec<KBucket>,
}

impl RoutingTable {
    pub fn new(local_id: NodeId) -> Self {
        let buckets = (0..160).map(|_| KBucket::new()).collect();
        Self { local_id, buckets }
    }

    fn bucket_index(&self, node_id: &NodeId) -> usize {
        let xor_dist = self.local_id.xor_distance(node_id);
        let leading = xor_dist
            .iter()
            .enumerate()
            .find(|(_, &b)| b != 0)
            .map(|(i, &b)| i * 8 + (b.leading_zeros() as usize))
            .unwrap_or(160 * 8);
        (1280usize - 1).saturating_sub(leading).min(159)
    }

    pub fn insert(&mut self, entry: KBucketEntry) -> InsertResult {
        let idx = self.bucket_index(&entry.node_id);
        let result = self.buckets[idx].insert(entry.clone());
        if result == InsertResult::Full && self.can_split(idx) {
            let split_bit = idx;
            let byte_idx = split_bit / 8;
            let bit_mask = 1 << (7 - (split_bit % 8));
            let dist = self.local_id.xor_distance(&entry.node_id);
            let bit = (dist[byte_idx] & bit_mask) != 0;
            let _ = self.split_bucket(idx);
            let new_idx = if bit { idx + 1 } else { idx };
            return self.buckets[new_idx].insert(entry);
        }
        result
    }

    pub fn find_closest(&self, target: &NodeId, count: usize) -> Vec<KBucketEntry> {
        let mut candidates = Vec::new();
        let idx = self.bucket_index(target);
        let num_buckets = self.buckets.len();

        for offset in 0..num_buckets {
            let lower = if idx >= offset {
                idx - offset
            } else {
                usize::MAX
            };
            let upper = idx + offset;

            if lower < num_buckets {
                candidates.extend(self.buckets[lower].entries().iter().cloned());
            }
            if upper < num_buckets && upper != lower {
                candidates.extend(self.buckets[upper].entries().iter().cloned());
            }
            if candidates.len() >= count {
                break;
            }
        }

        candidates.sort_by(|a, b| {
            let da = a.node_id.xor_distance(target);
            let db = b.node_id.xor_distance(target);
            da.cmp(&db)
        });

        candidates.truncate(count);
        candidates
    }

    pub fn refresh_list(&self) -> Vec<usize> {
        self.buckets
            .iter()
            .enumerate()
            .filter(|(_, b)| b.needs_refresh())
            .map(|(i, _)| i)
            .collect()
    }

    pub fn mark_refreshed(&mut self, index: usize) {
        if index < self.buckets.len() {
            self.buckets[index].mark_refreshed();
        }
    }

    pub fn size(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    pub fn remove(&mut self, node_id: &NodeId) {
        let idx = self.bucket_index(node_id);
        if idx < self.buckets.len() {
            self.buckets[idx].remove(node_id);
        }
    }

    pub fn local_id(&self) -> &NodeId {
        &self.local_id
    }

    pub fn buckets(&self) -> &[KBucket] {
        &self.buckets
    }

    fn can_split(&self, bucket_idx: usize) -> bool {
        let local_idx = self.bucket_index(&self.local_id);
        bucket_idx < self.buckets.len() && bucket_idx == local_idx
    }

    pub fn split_bucket(&mut self, bucket_idx: usize) -> bool {
        if bucket_idx >= self.buckets.len() {
            return false;
        }
        let (left, right) = self.buckets[bucket_idx].split(&self.local_id, bucket_idx);
        self.buckets[bucket_idx] = left;
        self.buckets.insert(bucket_idx + 1, right);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qvs_core::{generate_node_id, NodeId};

    fn make_entry(node_id: NodeId) -> KBucketEntry {
        KBucketEntry {
            node_id,
            addr: "127.0.0.1:8621".parse().unwrap(),
            last_seen: Instant::now(),
            latency: Duration::from_millis(10),
            is_firewalled: false,
        }
    }

    #[test]
    fn test_bucket_insert() {
        let mut bucket = KBucket::new();
        assert!(!bucket.is_full());
        let id1 = NodeId([1u8; 20]);
        let result = bucket.insert(make_entry(id1));
        assert_eq!(result, InsertResult::Added);
        assert_eq!(bucket.len(), 1);
    }

    #[test]
    fn test_bucket_full() {
        let mut bucket = KBucket::new();
        for i in 0..K {
            let mut id = [0u8; 20];
            id[0] = i as u8;
            bucket.insert(make_entry(NodeId(id)));
        }
        assert!(bucket.is_full());
        let new_id = NodeId([0xFF; 20]);
        let result = bucket.insert(make_entry(new_id));
        assert_eq!(result, InsertResult::Full);
    }

    #[test]
    fn test_bucket_existing() {
        let mut bucket = KBucket::new();
        let id = NodeId([1u8; 20]);
        bucket.insert(make_entry(id));
        let result = bucket.insert(make_entry(id));
        assert_eq!(result, InsertResult::Existing);
    }

    #[test]
    fn test_routing_table_insert_and_find() {
        let local = NodeId(generate_node_id());
        let mut table = RoutingTable::new(local);
        let target = NodeId(generate_node_id());
        table.insert(make_entry(target));

        let found = table.find_closest(&target, 5);
        assert!(!found.is_empty());
        assert_eq!(found[0].node_id, target);
    }

    #[test]
    fn test_routing_table_size() {
        let local = NodeId(generate_node_id());
        let mut table = RoutingTable::new(local);
        for i in 0..10 {
            let mut id = [0u8; 20];
            id[0] = i;
            table.insert(make_entry(NodeId(id)));
        }
        assert_eq!(table.size(), 8);
    }

    #[test]
    fn test_bucket_split() {
        let local = NodeId(generate_node_id());
        let mut table = RoutingTable::new(local);

        // Fill bucket 159 (where all typical entries land) to capacity
        for i in 0..K {
            let mut id = [0u8; 20];
            id[0] = (i % 256) as u8;
            id[1] = (i / 256) as u8;
            table.insert(make_entry(NodeId(id)));
        }

        // Add one more - should be Full since bucket can't split
        let extra = make_entry(NodeId([0xFF; 20]));
        let result = table.insert(extra);
        assert_eq!(result, InsertResult::Full);

        // can_split returns true for the local node's bucket only
        let local_idx = table.bucket_index(&local);
        assert!(table.can_split(local_idx));
        assert!(!table.can_split(159));
    }

    #[test]
    fn test_refresh_list() {
        let local = NodeId(generate_node_id());
        let table = RoutingTable::new(local);
        let list = table.refresh_list();
        assert!(list.len() <= 160);
    }
}
