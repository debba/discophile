//! Single-producer, single-consumer sample ring between the engine thread and the device callback.
//!
//! Indices grow forever (u64 samples) so markers can refer to absolute positions. Samples are stored
//! as `AtomicU32` bit patterns: the callback never locks, and a flush racing with a read can at worst
//! repeat a few stale samples instead of being undefined behaviour.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

struct Shared {
    slots: Box<[AtomicU32]>,
    mask: u64,
    write: AtomicU64,
    read: AtomicU64,
    /// The consumer jumps here when it is behind: everything before it was flushed.
    skip: AtomicU64,
}

pub struct Producer {
    shared: Arc<Shared>,
    /// Treat a pending flush as done even if the consumer has not acknowledged it yet.
    force: bool,
}

pub struct Consumer {
    shared: Arc<Shared>,
}

/// A ring holding at least `capacity` samples (rounded up to a power of two).
pub fn ring(capacity: usize) -> (Producer, Consumer) {
    let capacity = capacity.max(2).next_power_of_two();
    let slots = (0..capacity).map(|_| AtomicU32::new(0)).collect::<Vec<_>>().into_boxed_slice();
    let shared =
        Arc::new(Shared { slots, mask: capacity as u64 - 1, write: AtomicU64::new(0), read: AtomicU64::new(0), skip: AtomicU64::new(0) });
    (Producer { shared: shared.clone(), force: false }, Consumer { shared })
}

impl Producer {
    pub fn capacity(&self) -> usize {
        self.shared.slots.len()
    }

    /// Absolute index of the next sample to be written.
    pub fn write_index(&self) -> u64 {
        self.shared.write.load(Ordering::Relaxed)
    }

    /// Absolute index of the next sample the device will play.
    pub fn read_index(&self) -> u64 {
        let read = self.shared.read.load(Ordering::Acquire);
        let skip = self.shared.skip.load(Ordering::Relaxed);
        if self.force { read.max(skip) } else { read }
    }

    /// Whether the consumer has caught up with the last flush.
    pub fn flush_acknowledged(&self) -> bool {
        self.force || self.shared.read.load(Ordering::Acquire) >= self.shared.skip.load(Ordering::Relaxed)
    }

    /// Samples written but not yet played.
    pub fn buffered(&self) -> usize {
        let write = self.write_index();
        let read = self.read_index().max(self.shared.skip.load(Ordering::Relaxed));
        write.saturating_sub(read) as usize
    }

    pub fn free(&self) -> usize {
        let used = self.write_index().saturating_sub(self.read_index()) as usize;
        self.capacity().saturating_sub(used)
    }

    /// Writes as many samples as fit and returns how many were written.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let count = samples.len().min(self.free());
        let write = self.write_index();
        for (offset, sample) in samples[..count].iter().enumerate() {
            let slot = &self.shared.slots[((write + offset as u64) & self.shared.mask) as usize];
            slot.store(sample.to_bits(), Ordering::Relaxed);
        }
        self.shared.write.store(write + count as u64, Ordering::Release);
        count
    }

    /// Discards everything not yet played. Until the consumer acknowledges, free space stays
    /// conservative so unplayed slots are not overwritten while being read.
    pub fn flush(&mut self) {
        self.force = false;
        self.shared.skip.store(self.write_index(), Ordering::Release);
    }

    /// Stops waiting for the consumer to acknowledge a flush (for example when the device stalled).
    pub fn force_flush(&mut self) {
        self.force = true;
    }

    /// Clears the forced state once the consumer has caught up.
    pub fn settle(&mut self) {
        if self.force && self.shared.read.load(Ordering::Acquire) >= self.shared.skip.load(Ordering::Relaxed) {
            self.force = false;
        }
    }
}

impl Consumer {
    /// Reads up to `out.len()` samples and returns how many were read.
    pub fn pop(&mut self, out: &mut [f32]) -> usize {
        let mut read = self.shared.read.load(Ordering::Relaxed);
        let skip = self.shared.skip.load(Ordering::Acquire);
        if skip > read {
            read = skip;
        }
        let write = self.shared.write.load(Ordering::Acquire);
        let count = out.len().min(write.saturating_sub(read) as usize);
        for (offset, sample) in out[..count].iter_mut().enumerate() {
            let slot = &self.shared.slots[((read + offset as u64) & self.shared.mask) as usize];
            *sample = f32::from_bits(slot.load(Ordering::Relaxed));
        }
        self.shared.read.store(read + count as u64, Ordering::Release);
        count
    }

    /// Acknowledges a flush without reading (used while output is silent).
    pub fn skip_flushed(&mut self) {
        let read = self.shared.read.load(Ordering::Relaxed);
        let skip = self.shared.skip.load(Ordering::Acquire);
        if skip > read {
            self.shared.read.store(skip, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_pop_in_order_across_wraparound() {
        let (mut producer, mut consumer) = ring(8);
        let mut out = [0.0; 5];
        for round in 0..10 {
            let input: Vec<f32> = (0..5).map(|i| (round * 5 + i) as f32).collect();
            assert_eq!(producer.push(&input), 5);
            assert_eq!(consumer.pop(&mut out), 5);
            assert_eq!(&out[..], &input[..]);
        }
        assert_eq!(producer.read_index(), 50);
    }

    #[test]
    fn push_stops_when_full() {
        let (mut producer, mut consumer) = ring(8);
        assert_eq!(producer.push(&[1.0; 6]), 6);
        assert_eq!(producer.push(&[2.0; 6]), 2);
        assert_eq!(producer.free(), 0);
        let mut out = [0.0; 3];
        assert_eq!(consumer.pop(&mut out), 3);
        assert_eq!(producer.free(), 3);
        assert_eq!(producer.buffered(), 5);
    }

    #[test]
    fn pop_returns_short_read_when_empty() {
        let (mut producer, mut consumer) = ring(16);
        producer.push(&[1.0, 2.0]);
        let mut out = [9.0; 4];
        assert_eq!(consumer.pop(&mut out), 2);
        assert_eq!(out, [1.0, 2.0, 9.0, 9.0]);
    }

    #[test]
    fn flush_discards_unplayed_samples_after_acknowledgement() {
        let (mut producer, mut consumer) = ring(8);
        producer.push(&[1.0; 8]);
        producer.flush();
        assert!(!producer.flush_acknowledged());
        // Old samples still occupy the ring until the consumer jumps past them.
        assert_eq!(producer.free(), 0);
        let mut out = [0.0; 4];
        assert_eq!(consumer.pop(&mut out), 0);
        assert!(producer.flush_acknowledged());
        assert_eq!(producer.free(), 8);
        producer.push(&[3.0, 4.0]);
        assert_eq!(consumer.pop(&mut out), 2);
        assert_eq!(&out[..2], &[3.0, 4.0]);
    }

    #[test]
    fn forced_flush_frees_space_immediately() {
        let (mut producer, mut consumer) = ring(8);
        producer.push(&[1.0; 8]);
        producer.flush();
        producer.force_flush();
        assert_eq!(producer.free(), 8);
        producer.push(&[5.0; 3]);
        let mut out = [0.0; 8];
        assert_eq!(consumer.pop(&mut out), 3);
        assert_eq!(&out[..3], &[5.0; 3]);
        producer.settle();
        assert!(producer.flush_acknowledged());
    }

    #[test]
    fn threads_see_every_sample_once() {
        let (mut producer, mut consumer) = ring(64);
        let total = 20_000usize;
        let writer = std::thread::spawn(move || {
            let mut next = 0usize;
            while next < total {
                let chunk: Vec<f32> = (next..(next + 7).min(total)).map(|v| v as f32).collect();
                next += producer.push(&chunk);
                std::thread::yield_now();
            }
        });
        let mut expected = 0usize;
        let mut out = [0.0f32; 13];
        while expected < total {
            let n = consumer.pop(&mut out);
            for value in &out[..n] {
                assert_eq!(*value as usize, expected);
                expected += 1;
            }
            std::thread::yield_now();
        }
        writer.join().unwrap();
    }
}
