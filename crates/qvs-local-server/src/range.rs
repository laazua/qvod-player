#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeResult {
    pub start: u64,
    pub end: u64,
    pub total_length: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeHeader {
    pub ranges: Vec<RangeResult>,
}

impl RangeHeader {
    #[must_use]
    pub fn parse(header: &str, total_length: u64) -> Option<Self> {
        let header = header.strip_prefix("bytes=")?;
        let ranges: Vec<RangeResult> = header
            .split(',')
            .filter_map(|part| {
                let part = part.trim();
                Self::parse_range(part, total_length)
            })
            .collect();
        if ranges.is_empty() {
            return None;
        }
        Some(Self { ranges })
    }

    fn parse_range(spec: &str, total_length: u64) -> Option<RangeResult> {
        if total_length == 0 {
            return None;
        }
        if let Some((start_str, end_str)) = spec.split_once('-') {
            if start_str.is_empty() {
                let suffix: u64 = end_str.parse().ok()?;
                if suffix == 0 {
                    return None;
                }
                let start = total_length.saturating_sub(suffix);
                let end = total_length - 1;
                Some(RangeResult {
                    start,
                    end,
                    total_length,
                })
            } else {
                let start: u64 = start_str.parse().ok()?;
                if end_str.is_empty() {
                    let end = total_length - 1;
                    Some(RangeResult {
                        start,
                        end,
                        total_length,
                    })
                } else {
                    let end: u64 = end_str.parse().ok()?;
                    let end = end.min(total_length - 1);
                    Some(RangeResult {
                        start,
                        end,
                        total_length,
                    })
                }
            }
        } else {
            None
        }
    }

    #[must_use]
    pub fn content_range(&self) -> String {
        if let Some(range) = self.ranges.first() {
            format!("bytes {}-{}/{}", range.start, range.end, range.total_length)
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_range() {
        let rh = RangeHeader::parse("bytes=0-499", 1000).unwrap();
        assert_eq!(rh.ranges.len(), 1);
        assert_eq!(rh.ranges[0].start, 0);
        assert_eq!(rh.ranges[0].end, 499);
    }

    #[test]
    fn test_parse_open_ended_range() {
        let rh = RangeHeader::parse("bytes=500-", 1000).unwrap();
        assert_eq!(rh.ranges[0].start, 500);
        assert_eq!(rh.ranges[0].end, 999);
    }

    #[test]
    fn test_parse_suffix_range() {
        let rh = RangeHeader::parse("bytes=-500", 1000).unwrap();
        assert_eq!(rh.ranges[0].start, 500);
        assert_eq!(rh.ranges[0].end, 999);
    }

    #[test]
    fn test_content_range() {
        let rh = RangeHeader::parse("bytes=200-499", 1000).unwrap();
        assert_eq!(rh.content_range(), "bytes 200-499/1000");
    }

    #[test]
    fn test_invalid_header() {
        assert!(RangeHeader::parse("invalid", 1000).is_none());
        assert!(RangeHeader::parse("", 1000).is_none());
    }
}
