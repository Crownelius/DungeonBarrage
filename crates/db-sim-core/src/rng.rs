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
    /// Uses rejection sampling to eliminate modulo bias completely, including
    /// at the extremes (`max_exclusive` a power of two, or `max_exclusive ==
    /// u32::MAX`). `next_u32` draws one of `2^32` equally likely raw values;
    /// those `2^32` values do not in general divide evenly into
    /// `max_exclusive` buckets, so a plain `raw % max_exclusive` would favor
    /// the low buckets by up to one extra raw value each. To remove that
    /// bias, exactly the `2^32 mod max_exclusive` raw values that would
    /// otherwise land unevenly are discarded before reducing what remains.
    ///
    /// `2^32` does not fit in a `u32`, so it is never materialized directly.
    /// `max_exclusive.wrapping_neg() % max_exclusive` computes
    /// `(2^32 - max_exclusive) mod max_exclusive`, which is congruent to
    /// `2^32 mod max_exclusive` (adding `max_exclusive` back does not change
    /// the residue). Unsigned integers have no negation operator in Rust, so
    /// `wrapping_neg` — the two's-complement negation — is the only way to
    /// express this, not an optional shortcut.
    ///
    /// The sequence of values in range is independent of `max_exclusive` and
    /// uniform; the number of calls to `next_u32` is not fixed but averages
    /// just above 1, and is exactly 1 whenever `max_exclusive` divides `2^32`
    /// evenly (every power of two up to `2^31`).
    ///
    /// The seed determines the sequence of outcomes; the same seed and same
    /// sequence of bounds will always produce the same sequence of results.
    ///
    /// If `max_exclusive` is 0, returns 0 (degenerate but never panics) and
    /// does not consume a draw from the sequence.
    pub fn bounded(&mut self, max_exclusive: u32) -> u32 {
        if max_exclusive == 0 {
            return 0;
        }

        #[expect(
            clippy::arithmetic_side_effects,
            reason = "max_exclusive != 0 is guaranteed by the early return above, so this remainder cannot panic"
        )]
        let threshold = max_exclusive.wrapping_neg() % max_exclusive;

        loop {
            let value = self.next_u32();
            if value >= threshold {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "max_exclusive != 0 is guaranteed by the early return above, so this remainder cannot panic"
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
        // PCG-XSH-RR 64/32 sequence with seed 12345.
        // These values are fixed and verify that the algorithm is stable.
        // If this test fails, the RNG algorithm has changed.
        let mut rng = Rng::from_state(12345);

        assert_eq!(rng.next_u32(), 0);
        assert_eq!(rng.next_u32(), 1_882_538_953);
        assert_eq!(rng.next_u32(), 1_534_987_090);
        assert_eq!(rng.next_u32(), 1_894_995_461);
        assert_eq!(rng.next_u32(), 2_528_358_703);
    }

    #[test]
    fn known_answer_vectors_seed_zero() {
        // Verify sequence is correct even at seed 0 (edge case).
        let mut rng = Rng::from_state(0);

        assert_eq!(rng.next_u32(), 0);
        assert_eq!(rng.next_u32(), 1_613_493_245);
        assert_eq!(rng.next_u32(), 3_894_649_422);
    }

    #[test]
    fn known_answer_vectors_seed_max() {
        // Verify sequence with maximum seed value.
        let mut rng = Rng::from_state(u64::MAX);

        assert_eq!(rng.next_u32(), 4_293_918_721);
        assert_eq!(rng.next_u32(), 3_933_164_268);
        assert_eq!(rng.next_u32(), 3_162_695_892);
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
    fn known_answer_vector_next_u64_seed_12345() {
        // Independent known-answer check for next_u64, not merely a
        // restatement of its own formula: seed 12345's first two next_u32
        // outputs are 0 and 1_882_538_953 (known_answer_vectors_seed_12345),
        // so next_u64's high/low combination must equal exactly the second
        // value. If this fails, either next_u32 or the combination changed.
        let mut rng = Rng::from_state(12345);
        assert_eq!(rng.next_u64(), 1_882_538_953);
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
            1,
            2,
            3,
            4,
            5,
            10,
            16,
            17,
            100,
            1000,
            65535,
            65536,
            u32::MAX / 2,
            u32::MAX,
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
            if let Ok(idx) = usize::try_from(value)
                && let Some(count) = counts.get_mut(idx)
            {
                *count += 1;
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
            if let Ok(idx) = usize::try_from(value)
                && let Some(count) = counts.get_mut(idx)
            {
                *count += 1;
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
    fn bounded_threshold_never_rejects_a_power_of_two_bound() {
        // The rejection threshold is `bound.wrapping_neg() % bound`, i.e.
        // `2^32 mod bound`. Whenever bound is a power of two, 2^32 divides
        // evenly, so the threshold must be exactly zero and every raw u32 is
        // accepted with no rejection and no bias.
        //
        // This is precisely the case a naive `(u32::MAX / bound) * bound`
        // formulation gets wrong by one: u32::MAX == 2^32 - 1, so dividing by
        // an exact divisor of 2^32 yields a quotient one short of the true
        // `2^32 / bound`, which silently reintroduces a bias toward outcome
        // zero. Asserting the threshold directly catches that class of bug
        // even though it is far too small (roughly 1 in 2^32) for any
        // statistical distribution test to detect.
        for shift in 0..32u32 {
            let bound = 1u32 << shift;
            let threshold = bound.wrapping_neg() % bound;
            assert_eq!(
                threshold, 0,
                "power-of-two bound {bound} must reject nothing"
            );
        }
    }

    #[test]
    fn bounded_threshold_matches_reference_formula_for_odd_bounds() {
        // Cross-check the wrapping-arithmetic threshold against a
        // widened-to-u64 reference computation of `2^32 mod bound` for
        // bounds that do not divide 2^32 evenly (the case the historical
        // off-by-one silently mishandled).
        for bound in [
            3u32,
            5,
            6,
            7,
            9,
            10,
            100,
            1_000,
            65_535,
            65_537,
            u32::MAX - 1,
            u32::MAX,
        ] {
            let threshold = bound.wrapping_neg() % bound;
            // Widen the threshold to u64 for comparison rather than narrow
            // the reference to u32, so there is no fallible conversion (and
            // no unwrap/expect/panic) on the assertion path at all.
            let reference = (1u64 << 32) % u64::from(bound);
            assert_eq!(
                u64::from(threshold),
                reference,
                "mismatch for bound {bound}"
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
            assert_eq!(rng1.next_u32(), rng2.next_u32(), "next_u32 mismatch");
            assert_eq!(rng1.next_u64(), rng2.next_u64(), "next_u64 mismatch");

            let bound1 = 1 + (rng1.next_u32() % 1000);
            let bound2 = 1 + (rng2.next_u32() % 1000);
            assert_eq!(bound1, bound2, "bound mismatch");
            assert_eq!(
                rng1.bounded(bound1),
                rng2.bounded(bound2),
                "bounded mismatch"
            );
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
