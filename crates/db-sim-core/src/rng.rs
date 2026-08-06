//! Versioned, seeded pseudo-random number generator.
//!
//! The sole source of entropy in a match is the seed, threaded explicitly through
//! simulation state. No ambient randomness, wall-clock values, or threads are used.
//! The same seed always produces the same sequence on any platform.

/// Version of the RNG algorithm.
///
/// Incremented when a change to [`Rng::next_u32`], [`Rng::next_u64`], or
/// [`Rng::bounded`] would alter the sequence for a given seed. Every match
/// records which version it ran under.
pub const RNG_VERSION: u32 = 1;

/// PCG-XSH-RR 64/32: a high-quality, efficient, compact PRNG.
///
/// Implements PCG as specified in O'Neill, "PCG: A Family of Simple Fast
/// Space-Efficient Statistically Good Algorithms for Random Number
/// Generation" (2014).
///
/// Uses 64 bits of state and produces 32-bit outputs. Deterministic given
/// a seed; the same seed produces the same sequence on every platform and
/// every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rng {
    /// Internal LCG state.
    state: u64,
}

impl Rng {
    /// PCG increment constant: a large odd number.
    ///
    /// The period of the state is 2^64 when paired with the multiplier.
    const INCREMENT: u64 = 1_442_695_040_888_963_407;

    /// Constructs an RNG from a seed.
    ///
    /// The seed is directly used as the initial state. The same seed will
    /// always produce the same sequence.
    #[must_use]
    pub const fn from_state(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Returns the current internal state for serialization.
    ///
    /// Can be passed to `from_state` to resume the sequence at this point.
    #[must_use]
    pub const fn state(&self) -> u64 {
        self.state
    }

    /// Advances the RNG and returns the next 32-bit value.
    pub fn next_u32(&mut self) -> u32 {
        let old_state = self.state;

        // LCG state update: state = state * multiplier + increment.
        // Wrapping is intentional and essential to the PRNG design.
        self.state = old_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(Self::INCREMENT);

        // XSH-RR: xorshift high bits to decorrelate, then rotate right by
        // a value derived from the high bits. This permutation breaks up the
        // structured low bits that the LCG multiplier leaves behind.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "XSH-RR outputs exactly 32 bits from the 64-bit state"
        )]
        let xorshifted = (((old_state >> 18) ^ old_state) >> 27) as u32;
        let rot = (old_state >> 59) as u32;

        xorshifted.rotate_right(rot)
    }

    /// Advances the RNG and returns the next 64-bit value.
    ///
    /// Combines two successive 32-bit outputs.
    pub fn next_u64(&mut self) -> u64 {
        let high = u64::from(self.next_u32());
        let low = u64::from(self.next_u32());
        (high << 32) | low
    }

    /// Returns a uniformly distributed value in `[0, max_exclusive)`.
    ///
    /// Uses rejection sampling to eliminate modulo bias. The sequence of
    /// values in range is independent of `max_exclusive` and uniform; the
    /// number of calls to `next_u32` is not fixed but averages close to 1.
    ///
    /// The seed determines the sequence of outcomes; the same seed and same
    /// sequence of bounds will always produce the same sequence of results.
    ///
    /// If `max_exclusive` is 0, returns 0 (degenerate but never panics).
    pub fn bounded(&mut self, max_exclusive: u32) -> u32 {
        if max_exclusive == 0 {
            return 0;
        }

        // Rejection sampling: compute the largest multiple of max_exclusive
        // that fits within u32, then accept only values below it.
        //
        // This ensures every outcome in [0, max_exclusive) is equally likely.
        // Without this, modulo would slightly favor low-order values when
        // max_exclusive does not divide 2^32 evenly.
        #[expect(
            clippy::integer_division,
            reason = "rejection sampling threshold: we need the quotient, not a float"
        )]
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "wrapping_mul is correct for PRNG arithmetic"
        )]
        let threshold = (u32::MAX / max_exclusive).wrapping_mul(max_exclusive);

        loop {
            let value = self.next_u32();
            if value <= threshold {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "value <= threshold < max_exclusive guarantees no panic"
                )]
                {
                    return value % max_exclusive;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answer_vectors_seed_12345() {
        // Independently verified PCG-XSH-RR 64/32 sequence.
        let mut rng = Rng::from_state(12345);

        assert_eq!(rng.next_u32(), 2_043_243_150);
        assert_eq!(rng.next_u32(), 4_179_160_776);
        assert_eq!(rng.next_u32(), 2_132_416_013);
        assert_eq!(rng.next_u32(), 4_034_862_522);
        assert_eq!(rng.next_u32(), 1_867_031_131);
    }

    #[test]
    fn known_answer_vectors_seed_zero() {
        // Verify sequence is correct even at seed 0 (edge case).
        let mut rng = Rng::from_state(0);

        assert_eq!(rng.next_u32(), 0);
        assert_eq!(rng.next_u32(), 3_419_452_221);
        assert_eq!(rng.next_u32(), 2_020_348_162);
    }

    #[test]
    fn known_answer_vectors_seed_max() {
        // Verify sequence with maximum seed value.
        let mut rng = Rng::from_state(u64::MAX);

        assert_eq!(rng.next_u32(), 1_540_259_682);
        assert_eq!(rng.next_u32(), 3_638_951_230);
        assert_eq!(rng.next_u32(), 1_823_614_763);
    }

    #[test]
    fn state_round_trip() {
        let seed = 999_999_999u64;
        let rng1 = Rng::from_state(seed);
        assert_eq!(rng1.state(), seed);

        // Verify state changes after next_u32 and can be persisted.
        let mut rng2 = Rng::from_state(seed);
        rng2.next_u32();
        rng2.next_u32();
        let state2 = rng2.state();
        assert_ne!(state2, seed);

        // Continuing from state2 gives same result as continuing from seed.
        let mut rng3 = Rng::from_state(state2);
        let val3 = rng3.next_u32();

        let mut rng4 = Rng::from_state(seed);
        rng4.next_u32();
        rng4.next_u32();
        let val4 = rng4.next_u32();

        assert_eq!(val3, val4);
    }

    #[test]
    fn next_u64_combines_two_u32s() {
        // Verify next_u64 is the high and low parts of two successive u32s.
        let mut rng1 = Rng::from_state(555_555);
        let val64 = rng1.next_u64();

        let mut rng2 = Rng::from_state(555_555);
        let high = u64::from(rng2.next_u32());
        let low = u64::from(rng2.next_u32());
        let expected = (high << 32) | low;

        assert_eq!(val64, expected);
    }

    #[test]
    fn bounded_zero_returns_zero() {
        // Edge case: zero bound is treated as a special case.
        let mut rng = Rng::from_state(42);
        assert_eq!(rng.bounded(0), 0);
    }

    #[test]
    fn bounded_one_always_returns_zero() {
        // Only outcome for bound 1 is 0.
        let mut rng = Rng::from_state(555);
        for _ in 0..100 {
            assert_eq!(rng.bounded(1), 0);
        }
    }

    #[test]
    fn bounded_never_returns_out_of_range() {
        // Core guarantee: result is always < max_exclusive.
        let mut rng = Rng::from_state(777);

        // Test a variety of bounds including edge cases.
        #[expect(
            clippy::integer_division,
            reason = "test constant: u32::MAX / 2 is intentional"
        )]
        let bounds = [
            1, 2, 3, 4, 5, 10, 16, 17, 100, 1000, 65535, 65536, u32::MAX / 2, u32::MAX,
        ];

        for bound in &bounds {
            for _ in 0..1000 {
                let value = rng.bounded(*bound);
                assert!(
                    value < *bound,
                    "bounded({}) returned {} >= bound",
                    bound,
                    value
                );
            }
        }
    }

    #[test]
    fn bounded_distribution_sanity_check() {
        // Verify bounded produces a roughly uniform distribution, not
        // obviously biased toward particular outcomes.
        let mut rng = Rng::from_state(1234);
        let bound = 10;
        let mut counts = [0usize; 10];

        // 10,000 samples of bounded(10) should distribute roughly evenly.
        for _ in 0..10_000 {
            let value = rng.bounded(bound);
            if let Ok(idx) = usize::try_from(value) {
                counts.get_mut(idx).map(|count| *count += 1);
            }
        }

        // Allow ±40% margin: each bucket of 10 should see 600-1400 in 10k trials.
        for (i, &count) in counts.iter().enumerate() {
            assert!(
                (600..=1400).contains(&count),
                "outcome {} appears {} times; distribution may be biased",
                i,
                count
            );
        }
    }

    #[test]
    fn bounded_no_modulo_bias() {
        // Rejection sampling prevents modulo bias. For bound B where
        // 2^32 % B != 0, we should see no bias toward small values.
        //
        // With 32-bit output and bound 5 (2^32 % 5 = 1), naïve modulo would
        // slightly favor outcomes 0-3. Rejection sampling fixes this.
        let mut rng = Rng::from_state(9999);
        let bound = 5;
        let mut counts = [0usize; 5];

        for _ in 0..100_000 {
            let value = rng.bounded(bound);
            if let Ok(idx) = usize::try_from(value) {
                counts.get_mut(idx).map(|count| *count += 1);
            }
        }

        // Each of 5 outcomes should appear ~20,000 times in 100k trials.
        // Allow ±5% margin (19k-21k per outcome).
        for (i, &count) in counts.iter().enumerate() {
            assert!(
                (19_000..=21_000).contains(&count),
                "outcome {} appears {} times (expected ~20000)",
                i,
                count
            );
        }
    }

    #[test]
    fn determinism_same_seed_same_sequence() {
        // Two RNGs with the same seed produce identical sequences.
        let seed = 0xDEAD_BEEF_CAFE_BABEu64;

        let mut rng1 = Rng::from_state(seed);
        let mut rng2 = Rng::from_state(seed);

        for _ in 0..100 {
            assert_eq!(rng1.next_u32(), rng2.next_u32());
            assert_eq!(rng1.next_u64(), rng2.next_u64());

            let bound = 1 + (rng1.next_u32() % 1000);
            assert_eq!(rng1.bounded(bound), rng2.bounded(bound));
        }
    }

    #[test]
    fn different_seeds_different_sequences() {
        // Different seeds produce different sequences (with high probability).
        let seed1 = 111_111u64;
        let seed2 = 222_222u64;

        let mut rng1 = Rng::from_state(seed1);
        let mut rng2 = Rng::from_state(seed2);

        // Collect first 10 outputs from each.
        let vals1: Vec<u32> = (0..10).map(|_| rng1.next_u32()).collect();
        let vals2: Vec<u32> = (0..10).map(|_| rng2.next_u32()).collect();

        // Extremely unlikely to match by chance.
        assert_ne!(vals1, vals2);
    }
}
