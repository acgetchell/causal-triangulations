# Delaunay Periodic Realization Diagnostic

## Status and Reproduction

The repository review on 26 October 2026 found a blocking discrepancy in the pinned Delaunay 0.8.3 realization validator. This is separate from the
read-only `2d-cdt` reference audit and from the ensemble/reverse-support limitation in issue #316. No dependency source was changed.

The existing property test minimized the failure to `seed = 3291`, `attempts = 5`, `toroidal = true`, starting from `from_toroidal_cdt(4, 3)`.
The retained Proptest regression and `tests/regressions.rs::toroidal_seed_3291_preserves_embedding_after_five_moves` preserve those inputs and the requirement
that successful moves pass complete evolved-state validation. Reproduce with:

```bash
uv run --locked --group dev research-repo-tools toolchain run -- cargo test --release --test regressions toroidal_seed_3291 -- --nocapture
```

The sequence is successful `Move22`, successful `Move22`, successful `Move31Remove`, rejected `Move31Remove` (`GeometricViolation`), then successful `Move22`.
Full validation passes after the first four attempts and reports a Level 4 intersection after the fifth, at counts `V = 11`, `E = 33`, `F = 22` and slice
profile `[3, 4, 4]`. In the retained reproduction, the reported simplex keys were `SimplexKey(191v5)` and `SimplexKey(190v3)`; UUIDs vary between constructions.
The ordinary Rust gate ran 820 tests: 819 passed and this property test failed. That is failure evidence, not a passing validation result.

## Independent Geometric Evidence

The period vector is `(4, 3)`. Let `R(x)` denote the exact rational value of the binary64 number parsed from decimal literal `x`. All additions and
subtractions below are rational operations, including integer period shifts. The reported pair can be expressed in a common covering-space chart as
triangles `ABC` and `CDE`:

```text
A = (R(3.0054768045463973) - 4, R(1.9708567415410918) - 3)
B = (R(1.0022551552419197),     R(0.012991573136809986))
C = (R(3.024162370210636) - 4,  R(0.01790730372627647))
D = (R(2.9835695878950927) - 4, R(1.0256320223226238))
E = (R(2.0132087630627957) - 4, R(2.9841994386833512) - 3)
```

The shared vertex is `C`. Both triangles are nondegenerate. Define the exact signed determinant
`d(P,Q,X) = (Q.x-P.x)(X.y-P.y) - (Q.y-P.y)(X.x-P.x)`. The directed edge `C → A` separates the two triangles: `d(C,A,B) > 0`, whereas

```text
d(C,A,C) = 0
d(C,A,D) = -155496619338172094085809883081 / 2535301200456458802993406410752
d(C,A,E) = -1341034503269294463868162961781 / 1267650600228229401496703205376
```

The latter two values are approximately `-0.061332601944958764` and `-1.0578896921816257`. Convexity therefore proves that the intersection of these two
triangles in this chart is exactly their shared vertex `C`, not an overlap outside their shared face. No tolerance is used in this argument. Python's
standard-library `fractions.Fraction` applied to the listed binary64 inputs independently reproduces these signs and fractions.

A separate exact-rational separating-axis scan of all 22 retained triangles, over period translations in `[-3, 3]²`, found no positive-area intersections
among 154 pairs whose bounding boxes overlap. The stored offsets are in `{-1, 0, 1}` and base coordinates are in the fundamental domain, so translations
outside that range cannot overlap these boxes. This scan does not replace all topology, boundary-contact or orientation checks; the separating-line proof
above addresses the specific reported pair.

## Rounding Mechanism and Repair Boundary

The shared y coordinate is exactly `y = 80647326388869 / 4503599627370496`. Binary64 evaluation gives

```text
fl(fl(y + 3) - 3) - y = -1 / 4503599627370496 = -2^-52.
```

One stored chart uses `y` directly; the other first lifts it by `+3`, then translates by `-3` for comparison. The two paths can therefore assign distinct
rounded coordinates to the same periodic vertex identity. The negative y displacement places the translated point strictly on the interior side of the
`C → A` separating line, introducing a spurious sliver. An exact predicate applied to these already inconsistent rounded coordinates cannot recover the
original periodic identity.

The published source revision is `3fa8a9907101c523574a0d17991a1f652b5419dc`. Its
[realization code](https://github.com/acgetchell/delaunay/blob/3fa8a9907101c523574a0d17991a1f652b5419dc/src/triangulation/realization.rs)
first lifts simplex coordinates and subsequently translates the realized simplex. The
[translation operation](https://github.com/acgetchell/delaunay/blob/3fa8a9907101c523574a0d17991a1f652b5419dc/src/geometry/realization.rs)
uses `shift.mul_add(period, already_lifted_coordinate)`. This source path and the exact arithmetic witness support a periodic-rounding false positive.
The additional discrepancy between the local successful edit check and later global validation still needs upstream investigation.

A repair belongs in Delaunay's periodic realization representation/predicates: preserve canonical coordinates plus integer offsets through a shared chart
construction or equivalent exact arithmetic, make equivalent lifted identities agree, and retain rejection of actual overlapping controls. Verify both
local and global validation against the unchanged fixture and publish the corrected dependency before normal adoption here.
CDT must not copy the intersection algorithm, change these coordinates to an easier fixture, skip full validation, or turn the supported successful move
into an accepted failure to obtain a green suite. The regression remains blocking until the geometry owner resolves the discrepancy.
