use std::{collections::BTreeSet, ops::Range, time::Duration};

#[derive(Debug, Clone)]
struct FilledRange {
    start: u64,
    end: u64,
}

impl PartialEq for FilledRange {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
    }
}

impl Eq for FilledRange {}

impl PartialOrd for FilledRange {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FilledRange {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.start.cmp(&other.start)
    }
}

#[derive(Debug, Clone)]
pub struct RingBuffer {
    capacity: u64,
    data: Vec<u8>,
    file_offset: u64,
    filled: BTreeSet<FilledRange>,
    play_position: u64,
    total_size: u64,
}

impl RingBuffer {
    #[must_use]
    pub fn new(capacity: u64, total_size: u64) -> Self {
        Self {
            capacity,
            data: vec![0u8; capacity as usize],
            file_offset: 0,
            filled: BTreeSet::new(),
            play_position: 0,
            total_size,
        }
    }

    pub fn write(&mut self, offset: u64, data: &[u8]) {
        if offset >= self.total_size || data.is_empty() {
            return;
        }

        let end = (offset + data.len() as u64).min(self.total_size);
        let len = (end - offset) as usize;
        let data = &data[..len];

        if offset < self.file_offset {
            self.slide_to(offset);
        }

        if offset + data.len() as u64 > self.file_offset + self.capacity {
            let new_start = (offset + data.len() as u64).saturating_sub(self.capacity);
            let new_start = new_start.min(self.total_size.saturating_sub(self.capacity));
            self.slide_to(new_start);
        }

        let buf_offset = offset.saturating_sub(self.file_offset);
        if buf_offset < self.capacity {
            let write_len = (data.len() as u64).min(self.capacity - buf_offset) as usize;
            let range_start = buf_offset as usize;
            let range_end = range_start + write_len;
            if range_end <= self.data.len() {
                self.data[range_start..range_end].copy_from_slice(&data[..write_len]);
            }
        }

        self.merge_range(offset, offset + data.len() as u64);
    }

    #[must_use]
    pub fn read(&self, offset: u64, length: u64) -> Option<Vec<u8>> {
        if !self.is_range_filled(offset, length) {
            return None;
        }
        let buf_offset = offset.saturating_sub(self.file_offset);
        if buf_offset + length > self.capacity {
            return None;
        }
        let start = buf_offset as usize;
        let end = start + length as usize;
        if end > self.data.len() {
            return None;
        }
        Some(self.data[start..end].to_vec())
    }

    #[must_use]
    pub fn is_range_filled(&self, offset: u64, length: u64) -> bool {
        let end = offset + length;
        for range in &self.filled {
            if range.start <= offset && range.end >= end {
                return true;
            }
        }
        false
    }

    #[must_use]
    pub fn is_playable(&self) -> bool {
        let head_len = (self.total_size.min(262_144)).min(self.capacity);
        self.is_range_filled(0, head_len)
    }

    #[must_use]
    pub fn buffered_duration(&self, duration_ms: u64) -> Duration {
        if self.total_size == 0 || duration_ms == 0 {
            return Duration::ZERO;
        }
        let bytes_per_ms = self.total_size as f64 / duration_ms as f64;
        if bytes_per_ms <= 0.0 {
            return Duration::ZERO;
        }
        let buffered = self.filled_after(self.play_position);
        let ms = (buffered as f64 / bytes_per_ms) as u64;
        Duration::from_millis(ms)
    }

    fn filled_after(&self, offset: u64) -> u64 {
        let mut total = 0u64;
        for range in &self.filled {
            if range.start >= offset {
                total += range.end - range.start;
            } else if range.end > offset {
                total += range.end - offset;
            }
        }
        total
    }

    fn filled_before(&self, offset: u64) -> u64 {
        let mut total = 0u64;
        for range in &self.filled {
            if range.end <= offset {
                total += range.end - range.start;
            } else if range.start < offset {
                total += offset - range.start;
            } else {
                break;
            }
        }
        total
    }

    pub fn set_play_position(&mut self, position: u64) {
        self.play_position = position;
    }

    #[must_use]
    pub fn filled_percentage(&self) -> f64 {
        if self.total_size == 0 {
            return 1.0;
        }
        let total_filled: u64 = self.filled.iter().map(|r| r.end - r.start).sum();
        (total_filled as f64) / (self.total_size as f64)
    }

    pub fn clear(&mut self) {
        self.filled.clear();
        self.data.fill(0);
        self.file_offset = 0;
        self.play_position = 0;
    }

    fn slide_to(&mut self, new_offset: u64) {
        if new_offset == self.file_offset {
            return;
        }
        let shift = if new_offset > self.file_offset {
            (new_offset - self.file_offset) as usize
        } else {
            let shift_back = (self.file_offset - new_offset) as usize;
            if shift_back >= self.capacity as usize {
                self.data.fill(0);
                self.filled.clear();
                self.file_offset = new_offset;
                return;
            }
            let mut new_data = vec![0u8; self.capacity as usize];
            let copy_start = shift_back;
            let copy_end = (copy_start + self.data.len()).min(self.capacity as usize);
            let copy_len = copy_end - copy_start;
            if copy_len > 0 {
                new_data[copy_start..copy_end].copy_from_slice(&self.data[..copy_len]);
            }
            self.data = new_data;
            let shift = shift_back as i64;

            let mut new_filled = BTreeSet::new();
            for range in self.filled.iter() {
                let new_start = (range.start as i64 - shift).max(0) as u64;
                let new_end = (range.end as i64 - shift).max(0) as u64;
                if new_end > new_start {
                    new_filled.insert(FilledRange {
                        start: new_start,
                        end: new_end,
                    });
                }
            }
            self.filled = new_filled;
            self.file_offset = new_offset;
            return;
        };

        if shift >= self.capacity as usize {
            self.data.fill(0);
            self.filled.clear();
        } else {
            let new_len = self.capacity as usize - shift;
            let mut new_data = vec![0u8; self.capacity as usize];
            new_data[..new_len].copy_from_slice(&self.data[shift..shift + new_len]);
            self.data = new_data;
        }

        let mut new_filled = BTreeSet::new();
        for range in self.filled.iter() {
            let new_start = range.start.saturating_sub(new_offset);
            let new_end = range.end.saturating_sub(new_offset);
            if new_end > new_start && new_start < self.capacity {
                new_filled.insert(FilledRange {
                    start: new_start,
                    end: new_end.min(self.capacity),
                });
            }
        }
        self.filled = new_filled;
        self.file_offset = new_offset;
    }

    fn merge_range(&mut self, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let mut new_start = start;
        let mut new_end = end;

        let to_remove: Vec<Range<u64>> = self
            .filled
            .iter()
            .filter(|r| {
                let overlap = r.start <= new_end && r.end >= new_start;
                if overlap {
                    new_start = new_start.min(r.start);
                    new_end = new_end.max(r.end);
                }
                overlap
            })
            .map(|r| r.start..r.end)
            .collect();

        for r in to_remove {
            self.filled.remove(&FilledRange {
                start: r.start,
                end: r.end,
            });
        }

        self.filled.insert(FilledRange {
            start: new_start,
            end: new_end,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = RingBuffer::new(1024 * 1024, 10_000_000);
        assert!(!buf.is_playable());
        assert_eq!(buf.filled_percentage(), 0.0);
    }

    #[test]
    fn test_write_and_read() {
        let mut buf = RingBuffer::new(1024 * 1024, 10_000_000);
        let data = vec![0xABu8; 262_144];
        buf.write(0, &data);
        assert!(buf.is_playable());
        let read = buf.read(0, 100);
        assert_eq!(read, Some(vec![0xABu8; 100]));
    }

    #[test]
    fn test_read_unwritten_returns_none() {
        let buf = RingBuffer::new(1024 * 1024, 10_000_000);
        assert!(buf.read(0, 100).is_none());
    }

    #[test]
    fn test_buffered_duration() {
        let mut buf = RingBuffer::new(1024 * 1024, 1_000_000);
        buf.write(0, &vec![0u8; 500_000]);
        let dur = buf.buffered_duration(10_000);
        assert!(dur > Duration::ZERO);
    }

    #[test]
    fn test_clear() {
        let mut buf = RingBuffer::new(1024 * 1024, 10_000_000);
        buf.write(0, &vec![0xABu8; 262_144]);
        assert!(buf.is_playable());
        buf.clear();
        assert!(!buf.is_playable());
    }

    #[test]
    fn test_non_sequential_write() {
        let mut buf = RingBuffer::new(1024 * 1024, 10_000_000);
        buf.write(1_000_000, &vec![0xABu8; 100]);
        assert!(!buf.is_playable());
        let read = buf.read(1_000_000, 100);
        assert!(read.is_some());
    }

    #[test]
    fn test_set_play_position() {
        let mut buf = RingBuffer::new(1024 * 1024, 10_000_000);
        buf.set_play_position(500_000);
    }

    #[test]
    fn test_empty_write() {
        let mut buf = RingBuffer::new(1024, 10_000_000);
        buf.write(0, &[]);
        assert_eq!(buf.filled_percentage(), 0.0);
    }
}
