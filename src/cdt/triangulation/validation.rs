#![forbid(unsafe_code)]

//! Whole-triangulation validation and causality checks.

use super::CdtTriangulation;
use crate::cdt::foliation::FoliationError;
use crate::errors::{
    CdtError, CdtResult, CdtValidationCheck, CdtValidationFailure, DelaunayValidationLevel,
};
use crate::geometry::DelaunayBackend2D;
use crate::geometry::backends::delaunay::DelaunayError;
use crate::geometry::traits::{TriangulationQuery, exactly_three};
use std::num::NonZeroUsize;

/// Named invariant sets for validating a CDT triangulation.
///
/// Every profile requires a structurally valid, nondegenerate, non-overlapping
/// embedding together with valid CDT topology, foliation, causality, and strict
/// simplex classification. [`Self::InitialDelaunay`] and
/// [`Self::StrictDelaunay`] additionally require the Level 5 Delaunay
/// empty-circumsphere predicate, while [`Self::Evolved`] intentionally does not.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::triangulation::*;
///
/// fn main() -> CdtResult<()> {
///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
///     tri.validate_with_profile(CdtValidationProfile::InitialDelaunay)?;
///     tri.validate_with_profile(CdtValidationProfile::Evolved)?;
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CdtValidationProfile {
    /// Initial Delaunay-backed CDT state before Monte Carlo evolution.
    InitialDelaunay,
    /// Evolved CDT state, which need not remain Delaunay.
    Evolved,
    /// Optional strict validation requiring an evolved state to remain Delaunay.
    StrictDelaunay,
}

/// Records whether the current embedding was checked at the backend mutation boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddingValidationState {
    Required,
    #[cfg(debug_assertions)]
    AlreadyValidated,
}

impl CdtTriangulation<DelaunayBackend2D> {
    /// Validate post-construction CDT properties.
    ///
    /// This is the invariant set required after ergodic moves and completed
    /// simulations: upstream straight-line embedding validity plus CDT topology,
    /// foliation, causality, and simplex-classification checks. It intentionally
    /// does not require the Level 5 Delaunay empty-circumsphere predicate,
    /// because local CDT moves are not expected to preserve Delaunay-ness.
    ///
    /// Constructors that create initial simulation meshes perform the stricter
    /// Level 1-5 Delaunay validation before returning.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::DelaunayValidationFailed`] if the backend fails
    /// upstream Level 1-4 embedding validation. Returns [`CdtError::Foliation`]
    /// with [`FoliationError::MissingBookkeeping`] when no foliation is present,
    /// returns [`CdtError::ValidationFailed`] if causality or
    /// simplex-classification checks fail, and returns topology or foliation
    /// errors from the corresponding validators when those invariants are
    /// violated.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     tri.validate()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate(&self) -> CdtResult<()> {
        self.validate_with_profile(CdtValidationProfile::Evolved)
    }

    /// Validates this triangulation against a named CDT invariant profile.
    ///
    /// The evolved profile requires straight-line embedding validity and all
    /// CDT-domain invariants without requiring the Delaunay empty-circumsphere
    /// predicate. The initial and strict-Delaunay profiles add that predicate.
    /// In every profile, embedding and strict causal simplex validity are
    /// independent mandatory checks.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::DelaunayValidationFailed`] when the selected geometry
    /// predicate fails. Returns [`CdtError::Foliation`] with
    /// [`FoliationError::MissingBookkeeping`] when no foliation is present,
    /// returns [`CdtError::ValidationFailed`] for causality or strict
    /// simplex-classification failures, and propagates topology or foliation
    /// errors from their corresponding validators.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_toroidal_cdt(4, 3)?;
    ///     tri.validate_with_profile(CdtValidationProfile::StrictDelaunay)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_with_profile(&self, profile: CdtValidationProfile) -> CdtResult<()> {
        self.validate_profile(profile, EmbeddingValidationState::Required)
    }

    /// Configures how often simulation runs perform full evolved-state validation.
    ///
    /// This reuses the Delaunay crate's global check policy as the cadence knob:
    /// `Some(n)` means validate the full CDT evolved-state contract after every
    /// `n` accepted local mutations; `None` means skip cadence checks and rely on
    /// mandatory final validation at checkpoint/result construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::num::NonZeroUsize;
    ///
    /// let mut tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    /// tri.set_delaunay_check_interval(NonZeroUsize::new(8));
    /// # Ok::<(), causal_triangulations::CdtError>(())
    /// ```
    pub fn set_delaunay_check_interval(&mut self, interval: Option<NonZeroUsize>) {
        self.geometry.set_delaunay_check_interval(interval);
    }

    /// Validates the post-move/final CDT invariant contract.
    pub(crate) fn validate_evolved_cdt(&self) -> CdtResult<()> {
        self.validate_with_profile(CdtValidationProfile::Evolved)
    }

    /// Validates either a foliated CDT state or an explicitly unfoliated geometry state.
    ///
    /// Raw point-cloud constructors are retained for geometry tests and experiments,
    /// but they are not valid inputs to a named CDT profile because causality cannot
    /// be evaluated without foliation bookkeeping.
    pub(crate) fn validate_supported_state(&self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return self.validate_unfoliated_geometry(EmbeddingValidationState::Required);
        }
        self.validate_evolved_cdt()
    }

    /// Completes evolved-state validation from the backend embedding result.
    #[cfg(test)]
    fn validate_evolved_cdt_after_embedding(
        &self,
        embedding_validation: Result<(), DelaunayError>,
    ) -> CdtResult<()> {
        self.validate_embedding_result(embedding_validation)?;
        self.validate_evolved_cdt_domain()
    }

    /// Applies the selected profile with explicit backend embedding evidence.
    fn validate_profile(
        &self,
        profile: CdtValidationProfile,
        embedding_state: EmbeddingValidationState,
    ) -> CdtResult<()> {
        self.require_profile_foliation()?;
        self.validate_geometry_for_profile(profile, embedding_state)?;
        self.validate_evolved_cdt_domain()
    }

    /// Rejects named CDT profiles when their causal invariant cannot be evaluated.
    fn require_profile_foliation(&self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return Err(FoliationError::MissingBookkeeping.into());
        }
        Ok(())
    }

    /// Validates the geometry and topology contract for raw unfoliated experiments.
    fn validate_unfoliated_geometry(
        &self,
        embedding_state: EmbeddingValidationState,
    ) -> CdtResult<()> {
        debug_assert!(self.foliation.is_none());
        self.validate_geometry_for_profile(CdtValidationProfile::Evolved, embedding_state)?;
        self.validate_topology()
    }

    /// Validates the geometry predicate selected by the CDT profile.
    fn validate_geometry_for_profile(
        &self,
        profile: CdtValidationProfile,
        embedding_state: EmbeddingValidationState,
    ) -> CdtResult<()> {
        match profile {
            CdtValidationProfile::Evolved => {
                if embedding_state == EmbeddingValidationState::Required {
                    self.validate_embedding_result(self.geometry.validate_embedding())?;
                }
            }
            CdtValidationProfile::InitialDelaunay | CdtValidationProfile::StrictDelaunay => {
                self.geometry.validate_delaunay().map_err(|err| {
                    CdtError::DelaunayValidationFailed {
                        level: DelaunayValidationLevel::Five,
                        detail: validation_detail(err),
                    }
                })?;
            }
        }
        Ok(())
    }

    /// Maps backend embedding diagnostics into the public CDT error contract.
    fn validate_embedding_result(
        &self,
        embedding_validation: Result<(), DelaunayError>,
    ) -> CdtResult<()> {
        embedding_validation.map_err(|err| CdtError::DelaunayValidationFailed {
            level: DelaunayValidationLevel::Four,
            detail: format!(
                "{}; triangulation counts: V={}, E={}, F={}",
                validation_detail(err),
                self.geometry.vertex_count(),
                self.geometry.edge_count(),
                self.geometry.face_count(),
            ),
        })
    }

    /// Validates CDT-owned invariants after a realized backend mutation succeeds.
    ///
    /// The Delaunay backend's mutation boundary guarantees structural and
    /// straight-line realization validity before returning success. Repeating
    /// the global Level 4 scan here would make every proposal pay for the same
    /// whole-mesh predicate twice, so move finalization checks only the
    /// CDT-owned topology, foliation, causality, and simplex-classification
    /// contract. A foliated move still enters the evolved profile with explicit
    /// evidence that its embedding predicate has already passed. Raw unfoliated
    /// geometry experiments instead retain their geometry/topology-only contract.
    #[cfg(debug_assertions)]
    pub(crate) fn validate_after_realized_mutation(&self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return self.validate_unfoliated_geometry(EmbeddingValidationState::AlreadyValidated);
        }
        self.validate_profile(
            CdtValidationProfile::Evolved,
            EmbeddingValidationState::AlreadyValidated,
        )
    }

    /// Validates invariants owned by the CDT domain layer.
    fn validate_evolved_cdt_domain(&self) -> CdtResult<()> {
        self.require_profile_foliation()?;
        self.validate_topology()?;
        self.validate_foliation()?;
        self.validate_causality()?;
        self.validate_simplex_classification()?;

        Ok(())
    }

    /// Validates the initialization contract for labeled CDT constructors.
    ///
    /// Initial meshes must be genuine Delaunay triangulations and must satisfy
    /// the stricter CDT foliation, topology, causality, and simplex-classification
    /// invariants before any simulation code can observe them.
    pub(crate) fn validate_initial_delaunay_cdt(&mut self) -> CdtResult<()> {
        self.require_profile_foliation()?;
        self.validate_geometry_for_profile(
            CdtValidationProfile::InitialDelaunay,
            EmbeddingValidationState::Required,
        )?;
        self.validate_topology()?;
        self.validate_foliation()?;
        self.validate_causality()?;
        self.classify_all_simplices().map(|_| ())
    }

    /// Validate causality constraints.
    ///
    /// If no foliation is present, succeeds vacuously (no causal structure
    /// to check).  Otherwise delegates to [`validate_causality_delaunay`](Self::validate_causality_delaunay).
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::Foliation`] with
    /// [`FoliationError::StaleBookkeeping`]
    /// when stored foliation bookkeeping is stale. Returns
    /// [`CdtError::ValidationFailed`] when face vertices or labels cannot be
    /// resolved, and returns [`CdtError::CausalityViolation`] if any edge spans
    /// more than one time slice (`|Δt| > 1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     tri.validate_causality()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_causality(&self) -> CdtResult<()> {
        self.validate_causality_delaunay()
    }

    /// Validates the causal structure of this foliated triangulation.
    ///
    /// Reads time labels directly from face vertex data and checks that every
    /// triangle lies within a single slice pair. In a 2D triangulation, this
    /// implies that each edge of each finite face connects vertices within the
    /// same slice or adjacent slices, while avoiding backend-specific edge
    /// endpoint resolution.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::Foliation`] with
    /// [`FoliationError::StaleBookkeeping`]
    /// when stored foliation bookkeeping is stale (`has_current_foliation()` is
    /// false). Returns [`CdtError::ValidationFailed`] when a face cannot be
    /// resolved to three vertices, any face vertex is unlabeled, or a triangle
    /// lacks exactly one spacelike and two timelike edges. Returns
    /// [`CdtError::CausalityViolation`] if any triangle spans more than one time
    /// slice. Callers should handle or propagate all three categories.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::{CdtError, DelaunayValidationLevel};
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt).map_err(|err| {
    ///         CdtError::DelaunayValidationFailed {
    ///             level: DelaunayValidationLevel::Five,
    ///             detail: err.to_string(),
    ///         }
    ///     })?;
    ///     let tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)?;
    ///     tri.validate_causality_delaunay()?;
    ///     Ok(())
    /// }
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "causality validation includes detailed diagnostics for multiple face-resolution and label error paths"
    )]
    pub fn validate_causality_delaunay(&self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return Ok(());
        }
        if !self.has_current_foliation() {
            return Err(self.stale_foliation_error());
        }

        for face in self.geometry.faces() {
            let verts = self.geometry.face_vertices(&face).map_err(|err| {
                log::debug!(
                    "Causality validation failed to resolve vertices for face {:?}: {err}; vertex_count={}, edge_count={}, face_count={}",
                    face,
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                );
                CdtError::ValidationFailed {
                    check: CdtValidationCheck::Causality,
                    failure: CdtValidationFailure::FaceVerticesUnavailable {
                        face: format!("{:?}", face.simplex_key()),
                        detail: err.to_string(),
                    },
                }
            })?;

            let actual_vertex_count = verts.len();
            let Some([v0, v1, v2]) = exactly_three(verts) else {
                return Err(CdtError::ValidationFailed {
                    check: CdtValidationCheck::Causality,
                    failure: CdtValidationFailure::FaceVertexCount {
                        face: format!("{:?}", face.simplex_key()),
                        actual: actual_vertex_count,
                        expected: 3,
                    },
                });
            };

            let t0 = self
                .geometry
                .vertex_data_by_key(v0.vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        v0.vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: CdtValidationCheck::Causality,
                        failure: CdtValidationFailure::MissingVertexTimeLabel {
                            vertex: format!("{:?}", v0.vertex_key()),
                        },
                    }
                })?;
            let t1 = self
                .geometry
                .vertex_data_by_key(v1.vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        v1.vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: CdtValidationCheck::Causality,
                        failure: CdtValidationFailure::MissingVertexTimeLabel {
                            vertex: format!("{:?}", v1.vertex_key()),
                        },
                    }
                })?;
            let t2 = self
                .geometry
                .vertex_data_by_key(v2.vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        v2.vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: CdtValidationCheck::Causality,
                        failure: CdtValidationFailure::MissingVertexTimeLabel {
                            vertex: format!("{:?}", v2.vertex_key()),
                        },
                    }
                })?;

            let mut spacelike = 0;
            let mut timelike = 0;

            for (a, b) in [(t0, t1), (t1, t2), (t2, t0)] {
                let step_distance = self.time_step_distance(a, b);
                match step_distance {
                    0 => spacelike += 1,
                    1 => timelike += 1,
                    _ => {
                        return Err(CdtError::CausalityViolation {
                            time_0: a.min(b),
                            time_1: a.max(b),
                            step_distance,
                        });
                    }
                }
            }

            if !(spacelike == 1 && timelike == 2) {
                return Err(CdtError::ValidationFailed {
                    check: CdtValidationCheck::Causality,
                    failure: CdtValidationFailure::InvalidCdtTriangle {
                        face: format!("{:?}", face.simplex_key()),
                        spacelike_edges: spacelike,
                        timelike_edges: timelike,
                    },
                });
            }
        }

        Ok(())
    }
}

/// Extracts upstream validation diagnostics without duplicating wrapper context.
fn validation_detail(error: DelaunayError) -> String {
    match error {
        DelaunayError::ValidationFailed { detail, .. } => detail,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::foliation::EdgeType;
    use crate::config::CdtTopology;
    use crate::geometry::generators::build_delaunay2_with_data;
    use std::assert_matches;
    use std::num::NonZeroU32;

    /// Builds a minimal labeled Delaunay backend for validation tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt).expect("test Delaunay triangle should validate")
    }

    /// Builds intentionally unchecked metadata for causality validation tests.
    fn unchecked_open_boundary(
        backend: DelaunayBackend2D,
        time_slices: u32,
        dimension: u8,
    ) -> CdtTriangulation<DelaunayBackend2D> {
        CdtTriangulation::from_parts_before_validation(
            backend,
            time_slices,
            dimension,
            CdtTopology::OpenBoundary,
        )
        .expect("unchecked test metadata should use nonzero time slices")
    }

    /// Builds stable diagnostic text for seeded-triangulation comparisons.
    fn deterministic_triangle_debug_summary(backend: &DelaunayBackend2D) -> String {
        let mut vertices: Vec<_> = backend
            .vertices()
            .map(|vh| {
                let coords = backend.vertex_coordinates(&vh).map_or_else(
                    |err| format!("coord_error:{err}"),
                    |coords| format!("{coords:?}"),
                );
                format!(
                    "{:?}@{}:{:?}",
                    vh.vertex_key(),
                    coords,
                    backend.vertex_data_by_key(vh.vertex_key())
                )
            })
            .collect();
        vertices.sort_unstable();

        let mut edges: Vec<_> = backend
            .edges()
            .map(|edge| match backend.edge_endpoints(&edge) {
                Ok((v0, v1)) => format!(
                    "{:?}<->{:?}:{:?}->{:?}",
                    v0.vertex_key(),
                    v1.vertex_key(),
                    backend.vertex_data_by_key(v0.vertex_key()),
                    backend.vertex_data_by_key(v1.vertex_key())
                ),
                Err(error) => format!("endpoint_error:{error}"),
            })
            .collect();
        edges.sort_unstable();

        format!(
            "vertex_count={}, edge_count={}, face_count={}, is_valid={}, is_delaunay={}, vertices=[{}], edges=[{}]",
            backend.vertex_count(),
            backend.edge_count(),
            backend.face_count(),
            backend.is_valid(),
            backend.is_delaunay(),
            vertices.join(", "),
            edges.join(", "),
        )
    }

    #[test]
    fn named_profiles_reject_unfoliated_state() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 2, 2, 53)
            .expect("Failed to create triangulation");

        for profile in [
            CdtValidationProfile::InitialDelaunay,
            CdtValidationProfile::Evolved,
            CdtValidationProfile::StrictDelaunay,
        ] {
            assert_matches!(
                triangulation.validate_with_profile(profile),
                Err(CdtError::Foliation(FoliationError::MissingBookkeeping))
            );
        }
        assert_matches!(
            triangulation.validate(),
            Err(CdtError::Foliation(FoliationError::MissingBookkeeping))
        );
    }

    #[test]
    fn named_profiles_accept_initial_delaunay_state() {
        let triangulation =
            CdtTriangulation::from_cdt_strip(4, 3).expect("initial CDT strip should build");

        for profile in [
            CdtValidationProfile::InitialDelaunay,
            CdtValidationProfile::Evolved,
            CdtValidationProfile::StrictDelaunay,
        ] {
            triangulation
                .validate_with_profile(profile)
                .unwrap_or_else(|error| panic!("{profile:?} should accept initial state: {error}"));
        }
    }

    #[test]
    fn evolved_profile_rejects_noncausal_foliated_state() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 1.0], 1), ([0.0, 2.0], 2)])
            .expect("noncollinear triangle should build");
        let backend =
            DelaunayBackend2D::from_triangulation(dt).expect("single triangle should be Delaunay");
        let mut triangulation = CdtTriangulation::try_new(backend, 3, 2)
            .expect("triangle should enter unfoliated geometry state");
        triangulation
            .assign_foliation_by_y(NonZeroU32::new(3).expect("fixture has three nonempty slices"))
            .expect("y bands should establish current foliation bookkeeping");

        assert_matches!(
            triangulation.validate_with_profile(CdtValidationProfile::Evolved),
            Err(CdtError::CausalityViolation {
                time_0: 0,
                time_1: 2,
                step_distance: 2,
            })
        );

        let domain_error = triangulation
            .validate_evolved_cdt_after_embedding(Ok(()))
            .expect_err("successful embedding evidence must not bypass CDT-domain validation");
        assert_eq!(
            domain_error,
            CdtError::CausalityViolation {
                time_0: 0,
                time_1: 2,
                step_distance: 2,
            }
        );
    }

    #[test]
    fn validate_reports_embedding_failure_with_triangulation_counts() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let triangulation = CdtTriangulation::try_new(backend, 2, 2)
            .expect("triangle should enter unfoliated CDT state");

        let error = triangulation
            .validate_evolved_cdt_after_embedding(Err(DelaunayError::ValidationFailed {
                level: DelaunayValidationLevel::Four,
                detail: "non-adjacent simplices intersect".to_string(),
            }))
            .expect_err("embedding failure should reject evolved CDT state");

        assert_matches!(
            error,
            CdtError::DelaunayValidationFailed {
                level: DelaunayValidationLevel::Four,
                detail,
            } if detail
                == "non-adjacent simplices intersect; triangulation counts: V=3, E=3, F=1"
        );
    }

    #[test]
    fn validate_causality_is_vacuous_without_foliation() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 2, 2, 0x0A11_DA7E)
            .expect("Failed to create triangulation");

        triangulation
            .validate_causality()
            .expect("causality should pass without foliation");
    }

    #[test]
    fn causality_violation_detected() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build deterministic causal triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt)
            .expect("test Delaunay triangle should validate");
        let mut tri = unchecked_open_boundary(backend, 2, 2);

        tri.assign_foliation_by_y(NonZeroU32::new(2).expect("test slice count should be nonzero"))
            .expect("Should derive foliation from triangle coordinates");

        assert_eq!(
            tri.slice_sizes(),
            &[2, 1],
            "Deterministic triangle should assign slice sizes [2, 1], got {:?}; {}",
            tri.slice_sizes(),
            deterministic_triangle_debug_summary(tri.geometry())
        );
        tri.validate_causality_delaunay()
            .expect("deterministic causal triangle should start causally valid");
        assert!(
            tri.geometry().faces().any(|face| {
                tri.face_edge_types(&face).is_ok_and(|edge_types| {
                    edge_types
                        .is_some_and(|ets| ets.iter().any(|e| matches!(e, EdgeType::Timelike)))
                })
            }),
            "Deterministic causal triangle should contain a timelike edge; {}",
            deterministic_triangle_debug_summary(tri.geometry())
        );

        let vertex_to_mutate = tri
            .geometry()
            .vertices()
            .next()
            .expect("Deterministic causal triangle should contain a vertex");

        tri.set_vertex_data(&vertex_to_mutate, Some(3))
            .expect("Expected valid vertex handle while mutating deterministic triangle");

        assert_matches!(
            tri.validate_causality_delaunay(),
            Err(CdtError::Foliation(FoliationError::StaleBookkeeping { .. }))
        );
    }

    #[test]
    fn validate_causality_rejects_missing_live_label() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let vertex_to_clear = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex");

        tri.set_vertex_data(&vertex_to_clear, None)
            .expect("Expected valid vertex handle while clearing label");

        assert_matches!(
            tri.validate_causality_delaunay(),
            Err(CdtError::Foliation(FoliationError::StaleBookkeeping { .. }))
        );
    }

    #[test]
    fn validate_rejects_all_spacelike_triangle_before_causality_check() {
        let backend = labeled_triangle_backend([0, 0, 0]);
        let result = CdtTriangulation::from_labeled_delaunay(backend, 1, 2);

        assert_matches!(
            result,
            Err(CdtError::Foliation(
                FoliationError::SpacelikeOpenSliceEndpointCount {
                    slice: 0,
                    observed: 0,
                    expected: 2,
                }
            ))
        );
    }

    #[test]
    fn toroidal_causality_violation_reports_circular_step_distance() {
        let mut tri =
            CdtTriangulation::from_toroidal_cdt(3, 10).expect("build toroidal CDT (3, 10)");
        let slice0_vertex = tri
            .geometry()
            .vertices()
            .find(|vh| tri.geometry().vertex_data_by_key(vh.vertex_key()) == Some(0))
            .expect("Toroidal CDT should contain slice-0 vertices");

        tri.set_vertex_data(&slice0_vertex, Some(8))
            .expect("Expected valid vertex handle while mutating label");

        assert_matches!(
            tri.validate_causality_delaunay(),
            Err(CdtError::Foliation(FoliationError::StaleBookkeeping { .. }))
        );
    }
}
