#![forbid(unsafe_code)]

//! Whole-triangulation validation and causality checks.

use super::CdtTriangulation;
use crate::errors::{CdtError, CdtResult};
use crate::geometry::DelaunayBackend2D;
use crate::geometry::traits::TriangulationQuery;

impl CdtTriangulation<DelaunayBackend2D> {
    /// Validate CDT properties (geometry, Delaunay, topology, causality, foliation).
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] if backend geometry, Delaunay,
    /// causality, or cell-classification checks fail. Returns topology or
    /// foliation errors from the corresponding validators when those
    /// invariants are violated.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_seeded_points(5, 2, 2, 53)?;
    ///     tri.validate()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate(&self) -> CdtResult<()> {
        if !self.geometry.is_valid() {
            return Err(CdtError::ValidationFailed {
                check: "geometry".to_string(),
                detail: format!(
                    "triangulation is not valid (V={}, E={}, F={})",
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                ),
            });
        }

        if !self.geometry.is_delaunay() {
            return Err(CdtError::ValidationFailed {
                check: "Delaunay".to_string(),
                detail: format!(
                    "triangulation does not satisfy Delaunay property (V={}, E={}, F={})",
                    self.geometry.vertex_count(),
                    self.geometry.edge_count(),
                    self.geometry.face_count(),
                ),
            });
        }

        self.validate_topology()?;
        self.validate_foliation()?;
        self.validate_causality()?;
        self.validate_cell_classification()?;

        Ok(())
    }

    /// Validate causality constraints.
    ///
    /// If no foliation is present, succeeds vacuously (no causal structure
    /// to check).  Otherwise delegates to [`validate_causality_delaunay`](Self::validate_causality_delaunay).
    ///
    /// # Errors
    ///
    /// Returns error if any edge spans more than one time slice (`|Δt| > 1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt);
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
    /// Returns error if any triangle spans more than one time slice, if a face
    /// cannot be resolved to three vertices, or if any face vertex is unlabeled.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::geometry::*;
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let dt = build_delaunay2_with_data(&[
    ///         ([0.0, 0.0], 0),
    ///         ([1.0, 0.0], 0),
    ///         ([0.5, 1.0], 1),
    ///     ])?;
    ///     let backend = DelaunayBackend2D::from_triangulation(dt);
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
                    check: "causality".to_string(),
                    detail: "failed to resolve face vertices".to_string(),
                }
            })?;

            if verts.len() != 3 {
                return Err(CdtError::ValidationFailed {
                    check: "causality".to_string(),
                    detail: format!(
                        "face {:?} has {} vertices, expected 3",
                        face.cell_key(),
                        verts.len(),
                    ),
                });
            }

            let t0 = self
                .geometry
                .vertex_data_by_key(verts[0].vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        verts[0].vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: "causality".to_string(),
                        detail: format!(
                            "vertex {:?} has no time label in a foliated triangulation",
                            verts[0].vertex_key(),
                        ),
                    }
                })?;
            let t1 = self
                .geometry
                .vertex_data_by_key(verts[1].vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        verts[1].vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: "causality".to_string(),
                        detail: format!(
                            "vertex {:?} has no time label in a foliated triangulation",
                            verts[1].vertex_key(),
                        ),
                    }
                })?;
            let t2 = self
                .geometry
                .vertex_data_by_key(verts[2].vertex_key())
                .ok_or_else(|| {
                    log::debug!(
                        "Causality validation found unlabeled vertex {:?} while checking face {:?}",
                        verts[2].vertex_key(),
                        face,
                    );
                    CdtError::ValidationFailed {
                        check: "causality".to_string(),
                        detail: format!(
                            "vertex {:?} has no time label in a foliated triangulation",
                            verts[2].vertex_key(),
                        ),
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
                    check: "causality".to_string(),
                    detail: format!(
                        "invalid CDT triangle at face {:?}: spacelike={}, timelike={}",
                        face.cell_key(),
                        spacelike,
                        timelike
                    ),
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cdt::foliation::EdgeType;
    use crate::config::CdtTopology;
    use crate::geometry::generators::build_delaunay2_with_data;

    /// Builds a minimal labeled Delaunay backend for validation tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt)
    }

    /// Builds intentionally unchecked metadata for causality validation tests.
    fn unchecked_open_boundary(
        backend: DelaunayBackend2D,
        time_slices: u32,
        dimension: u8,
    ) -> CdtTriangulation<DelaunayBackend2D> {
        CdtTriangulation::wrap_unchecked(backend, time_slices, dimension, CdtTopology::OpenBoundary)
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
                Some((v0, v1)) => format!(
                    "{:?}<->{:?}:{:?}->{:?}",
                    v0.vertex_key(),
                    v1.vertex_key(),
                    backend.vertex_data_by_key(v0.vertex_key()),
                    backend.vertex_data_by_key(v1.vertex_key())
                ),
                None => "endpoint_error:unavailable".to_string(),
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
    fn validate_succeeds_for_known_good_seed() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 2, 2, 53)
            .expect("Failed to create triangulation");

        triangulation
            .validate()
            .expect("known good triangulation should validate");
    }

    #[test]
    fn validate_causality_is_vacuous_without_foliation() {
        let triangulation =
            CdtTriangulation::from_random_points(5, 2, 2).expect("Failed to create triangulation");

        triangulation
            .validate_causality()
            .expect("causality should pass without foliation");
    }

    #[test]
    fn causality_violation_detected() {
        let dt = build_delaunay2_with_data(&[([0.0, 0.0], 0), ([1.0, 0.0], 0), ([0.5, 1.0], 1)])
            .expect("Should build deterministic causal triangle");
        let backend = DelaunayBackend2D::from_triangulation(dt);
        let mut tri = unchecked_open_boundary(backend, 2, 2);

        tri.assign_foliation_by_y(2)
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
                tri.face_edge_types(&face)
                    .is_some_and(|ets| ets.iter().any(|e| matches!(e, EdgeType::Timelike)))
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

        match tri.validate_causality_delaunay() {
            Err(CdtError::CausalityViolation {
                time_0,
                time_1,
                step_distance,
            }) => {
                assert!(step_distance > 1);
                assert_eq!(step_distance, time_0.abs_diff(time_1));
            }
            other => panic!(
                "Expected CausalityViolation error, got {other:?}; {}",
                deterministic_triangle_debug_summary(tri.geometry())
            ),
        }
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

        assert!(matches!(
            tri.validate_causality_delaunay(),
            Err(CdtError::ValidationFailed { ref check, ref detail })
                if check == "causality"
                    && detail.contains("has no time label in a foliated triangulation")
        ));
    }

    #[test]
    fn validate_and_causality_reject_all_spacelike_triangle() {
        let backend = labeled_triangle_backend([0, 0, 0]);
        let tri = CdtTriangulation::from_labeled_delaunay(backend, 1, 2)
            .expect("single-slice labels should form foliation bookkeeping");

        for result in [tri.validate_causality_delaunay(), tri.validate()] {
            assert!(matches!(
                result,
                Err(CdtError::ValidationFailed { ref check, ref detail })
                    if check == "causality"
                        && detail.contains("invalid CDT triangle")
                        && detail.contains("spacelike=3")
                        && detail.contains("timelike=0")
            ));
        }
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

        match tri.validate_causality_delaunay() {
            Err(CdtError::CausalityViolation {
                time_0,
                time_1,
                step_distance,
            }) => {
                let raw = time_0.abs_diff(time_1);
                let circular = raw.min(10 - raw);
                assert_eq!(step_distance, circular);
                assert!(step_distance > 1);
                assert!(step_distance < raw);
            }
            other => panic!("Expected CausalityViolation on toroidal triangle, got {other:?}"),
        }
    }
}
