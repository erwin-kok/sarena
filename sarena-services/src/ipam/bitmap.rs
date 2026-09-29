use std::sync::Mutex;

struct State {
    words: Vec<u64>,
    count: usize,
}

pub struct AllocationBitmap {
    max: usize,
    state: Mutex<State>,
}

impl AllocationBitmap {
    pub fn new(max: usize) -> Self {
        Self {
            max,
            state: Mutex::new(State {
                words: vec![0u64; max.div_ceil(64)],
                count: 0,
            }),
        }
    }

    pub fn allocate(&self, offset: usize) -> bool {
        if offset >= self.max {
            return false;
        }
        let mut state = self.state();
        if get_bit(&state.words, offset) {
            return false;
        }
        set_bit(&mut state.words, offset);
        state.count += 1;
        true
    }

    pub fn allocate_next(&self) -> Option<usize> {
        let mut state = self.state();
        let next = allocate_bit(&state.words, self.max, state.count)?;
        set_bit(&mut state.words, next);
        state.count += 1;
        Some(next)
    }

    pub fn release(&self, offset: usize) {
        if offset >= self.max {
            return;
        }
        let mut state = self.state();
        if !get_bit(&state.words, offset) {
            return;
        }
        clear_bit(&mut state.words, offset);
        state.count -= 1;
    }

    pub fn for_each(&self, mut f: impl FnMut(usize)) {
        let state = self.state();
        for (word_idx, &word) in state.words.iter().enumerate() {
            let mut word = word;
            let mut bit = 0;
            while word > 0 {
                if word & 1 != 0 {
                    f(word_idx * 64 + bit);
                }
                bit += 1;
                word >>= 1;
            }
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("allocation bitmap lock poisoned")
    }

    #[cfg(test)]
    pub(crate) fn has(&self, offset: usize) -> bool {
        offset < self.max && get_bit(&self.state().words, offset)
    }

    #[cfg(test)]
    pub(crate) fn free(&self) -> usize {
        self.max - self.state().count
    }
}

fn allocate_bit(words: &[u64], max: usize, count: usize) -> Option<usize> {
    if count >= max {
        return None;
    }
    let offset = rand::random_range(0..max);
    (0..max)
        .map(|i| (offset + i) % max)
        .find(|&at| !get_bit(words, at))
}

fn get_bit(words: &[u64], offset: usize) -> bool {
    (words[offset / 64] >> (offset % 64)) & 1 == 1
}

fn set_bit(words: &mut [u64], offset: usize) {
    words[offset / 64] |= 1 << (offset % 64);
}

fn clear_bit(words: &mut [u64], offset: usize) {
    words[offset / 64] &= !(1 << (offset % 64));
}

#[cfg(test)]
fn count_ones(words: &[u64]) -> usize {
    words.iter().map(|w| w.count_ones() as usize).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocate_marks_offset_used_and_decrements_free() {
        let m = AllocationBitmap::new(4);
        assert_eq!(m.free(), 4);
        assert!(m.allocate(1));
        assert_eq!(m.free(), 3);
        assert!(m.has(1));
    }

    #[test]
    fn allocate_twice_fails_second_time() {
        let m = AllocationBitmap::new(4);
        assert!(m.allocate(0));
        assert!(!m.allocate(0));
    }

    #[test]
    fn allocate_out_of_range_fails() {
        let m = AllocationBitmap::new(4);
        assert!(!m.allocate(4));
        assert!(!m.has(4));
    }

    #[test]
    fn allocate_next_fills_then_reports_full() {
        let m = AllocationBitmap::new(2);
        let a = m.allocate_next().unwrap();
        let b = m.allocate_next().unwrap();
        assert_ne!(a, b);
        assert_eq!(m.free(), 0);
        assert_eq!(m.allocate_next(), None);
    }

    #[test]
    fn release_frees_offset_for_reuse() {
        let m = AllocationBitmap::new(1);
        assert!(m.allocate(0));
        assert_eq!(m.allocate_next(), None);
        m.release(0);
        assert_eq!(m.free(), 1);
        assert_eq!(m.allocate_next(), Some(0));
    }

    #[test]
    fn release_unallocated_is_a_noop() {
        let m = AllocationBitmap::new(4);
        m.release(2);
        assert_eq!(m.free(), 4);
    }

    #[test]
    fn for_each_visits_only_allocated_offsets() {
        let m = AllocationBitmap::new(8);
        m.allocate(1);
        m.allocate(5);
        let mut seen = Vec::new();
        m.for_each(|offset| seen.push(offset));
        assert_eq!(seen, vec![1, 5]);
    }

    #[test]
    fn spans_multiple_words() {
        let m = AllocationBitmap::new(70);
        assert!(m.allocate(63));
        assert!(m.allocate(64));
        assert!(m.allocate(69));
        assert!(m.has(63));
        assert!(m.has(64));
        assert!(m.has(69));
        assert!(!m.has(65));
    }

    #[test]
    fn count_ones_matches_expected_bit_counts() {
        assert_eq!(count_ones(&[0]), 0);
        assert_eq!(count_ones(&[0xff_ffff_ffff]), 40);
        assert_eq!(count_ones(&[0, 0, 1]), 1);
    }
}
