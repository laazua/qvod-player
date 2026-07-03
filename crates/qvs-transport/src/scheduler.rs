use std::collections::{BTreeMap, HashMap, HashSet};

use qvs_core::{Bitfield, BlockRequest, FileMeta, PiecePriority};

#[derive(Debug, Clone)]
pub struct PieceScheduler {
    metadata: FileMeta,
    playhead_piece: u32,
    download_queue: Vec<u32>,
    requested: HashSet<u32>,
    completed: HashSet<u32>,
    piece_priority_cache: HashMap<u32, PiecePriority>,
    seek_target: Option<u32>,
}

impl PieceScheduler {
    #[must_use]
    pub fn new(metadata: FileMeta) -> Self {
        Self {
            metadata,
            playhead_piece: 0,
            download_queue: Vec::new(),
            requested: HashSet::new(),
            completed: HashSet::new(),
            piece_priority_cache: HashMap::new(),
            seek_target: None,
        }
    }

    #[must_use]
    pub fn num_pieces(&self) -> u32 {
        let total = self.metadata.file_size + self.metadata.piece_length - 1;
        (total / self.metadata.piece_length) as u32
    }

    pub fn set_playhead(&mut self, piece: u32) {
        self.playhead_piece = piece;
    }

    pub fn mark_completed(&mut self, piece: u32) {
        self.completed.insert(piece);
        self.requested.remove(&piece);
    }

    pub fn mark_requested(&mut self, piece: u32) {
        self.requested.insert(piece);
    }

    #[must_use]
    pub fn is_completed(&self, piece: u32) -> bool {
        self.completed.contains(&piece)
    }

    #[must_use]
    pub fn is_requested(&self, piece: u32) -> bool {
        self.requested.contains(&piece)
    }

    pub fn set_seek_target(&mut self, piece_index: u32) {
        self.seek_target = Some(piece_index);
        self.playhead_piece = piece_index;
    }

    #[must_use]
    pub fn calculate_priority(&mut self, piece: u32) -> PiecePriority {
        if let Some(&cached) = self.piece_priority_cache.get(&piece) {
            return cached;
        }
        let dist = piece.abs_diff(self.playhead_piece);
        let priority = if piece == self.playhead_piece {
            PiecePriority::Critical
        } else if dist <= 2 {
            PiecePriority::High
        } else if dist <= 16 {
            PiecePriority::Normal
        } else {
            PiecePriority::Low
        };
        self.piece_priority_cache.insert(piece, priority);
        priority
    }

    pub fn update_download_queue(&mut self, peers: &[&Bitfield]) {
        let mut piece_counts: BTreeMap<u32, usize> = BTreeMap::new();
        let mut all_count: usize = 0;

        for peer_bitfield in peers {
            all_count += 1;
            for i in 0..self.num_pieces() {
                if peer_bitfield.has(i) {
                    *piece_counts.entry(i).or_default() += 1;
                }
            }
        }

        let mut scored: Vec<(u32, i64)> = Vec::new();
        for piece in 0..self.num_pieces() {
            if self.completed.contains(&piece) || self.requested.contains(&piece) {
                continue;
            }
            let rarity_score =
                all_count.saturating_sub(*piece_counts.get(&piece).unwrap_or(&0)) as i64;
            let priority = self.calculate_priority(piece);
            let priority_score = match priority {
                PiecePriority::Critical => 1000,
                PiecePriority::High => 100,
                PiecePriority::Normal => 10,
                PiecePriority::Low => 0,
            };
            let seek_bonus = if self
                .seek_target
                .is_some_and(|t| piece >= t.saturating_sub(2) && piece <= t + 2)
            {
                500
            } else {
                0
            };
            scored.push((piece, priority_score + rarity_score + seek_bonus));
        }

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        self.download_queue = scored.into_iter().map(|(p, _)| p).collect();
    }

    #[must_use]
    pub fn next_request(&self) -> Option<BlockRequest> {
        let piece = self.download_queue.first().copied()?;
        Some(BlockRequest {
            piece_index: piece,
            begin: 0,
            length: qvs_core::BLOCK_LENGTH as u32,
        })
    }

    #[must_use]
    pub fn select_peer_for_piece<'a>(
        &self,
        piece: u32,
        peers: &[(&'a [u8; 20], &Bitfield)],
    ) -> Option<&'a [u8; 20]> {
        peers
            .iter()
            .filter(|(_, bf)| bf.has(piece))
            .min_by_key(|(_, bf)| bf.count())
            .map(|(id, _)| *id)
    }

    #[must_use]
    pub fn rarest_first(&self, piece: u32, bitfields: &[&Bitfield]) -> u32 {
        let mut count = 0u32;
        for bf in bitfields {
            if bf.has(piece) {
                count += 1;
            }
        }
        count
    }

    #[must_use]
    pub fn download_queue(&self) -> &[u32] {
        &self.download_queue
    }

    #[must_use]
    pub fn playhead_piece(&self) -> u32 {
        self.playhead_piece
    }

    #[must_use]
    pub fn seek_target(&self) -> Option<u32> {
        self.seek_target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qvs_core::InfoHash;

    fn sample_meta() -> FileMeta {
        FileMeta {
            info_hash: InfoHash([0u8; 20]),
            filename: "test.mp4".into(),
            file_size: 1024 * 1024,
            piece_length: 256 * 1024,
            pieces: vec![],
            keyframe_index: None,
            duration_ms: 10000,
            video_codec: None,
            audio_codec: None,
            width: 1920,
            height: 1080,
            bitrate: 1000000,
            from_cache: false,
        }
    }

    #[test]
    fn test_scheduler_new() {
        let meta = sample_meta();
        let sched = PieceScheduler::new(meta);
        assert_eq!(sched.num_pieces(), 4);
    }

    #[test]
    fn test_priority_calculation() {
        let meta = sample_meta();
        let mut sched = PieceScheduler::new(meta);
        assert_eq!(sched.calculate_priority(0), PiecePriority::Critical);
        assert_eq!(sched.calculate_priority(1), PiecePriority::High);
        assert_eq!(sched.calculate_priority(3), PiecePriority::Normal);
    }

    #[test]
    fn test_download_queue_ordering() {
        let meta = sample_meta();
        let mut sched = PieceScheduler::new(meta);
        let bf = Bitfield::new(4);
        sched.update_download_queue(&[&bf]);
        assert!(!sched.download_queue().is_empty());
    }

    #[test]
    fn test_mark_completed() {
        let meta = sample_meta();
        let mut sched = PieceScheduler::new(meta);
        sched.mark_completed(0);
        assert!(sched.is_completed(0));
        assert!(!sched.is_completed(1));
    }

    #[test]
    fn test_seek_target() {
        let meta = sample_meta();
        let mut sched = PieceScheduler::new(meta);
        sched.set_seek_target(3);
        assert_eq!(sched.seek_target(), Some(3));
        assert_eq!(sched.playhead_piece(), 3);
    }

    #[test]
    fn test_rarest_first() {
        let meta = sample_meta();
        let sched = PieceScheduler::new(meta);
        let bf1 = Bitfield::new(4);
        let bf2 = Bitfield::new(4);
        assert_eq!(sched.rarest_first(0, &[&bf1, &bf2]), 0);
    }
}
