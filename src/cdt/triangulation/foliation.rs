#![forbid(unsafe_code)]

//! Foliation assignment, queries, and CDT simplex classification.

use super::CdtTriangulation;
use crate::cdt::foliation::{EdgeType, Foliation, FoliationError, SimplexType, classify_simplex};
use crate::config::CdtTopology;
use crate::errors::{
    BackendMutationOperation, BackendRollbackFailure, BackendRollbackFailures, CdtError, CdtResult,
    CdtValidationCheck, CdtValidationFailure, MeasurementCountField, TriangulationMetadataField,
};
use crate::geometry::backends::delaunay::{
    DelaunayEdgeHandle, DelaunayError, DelaunayFaceHandle, DelaunayVertexHandle,
};
use crate::geometry::traits::{TriangulationQuery, exactly_three};
use crate::geometry::{DelaunayBackend2D, SpacetimeCoordinate};
use crate::util::f64_band_to_u32;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroU32;

impl CdtTriangulation<DelaunayBackend2D> {
    /// Validate foliation consistency.
    ///
    /// If no foliation is present, succeeds vacuously.
    /// Otherwise checks:
    /// 1. The stored labeled-vertex count matches the geometry vertex count
    /// 2. Every stored time slice is non-empty
    /// 3. Live backend labels match stored per-slice bookkeeping
    ///
    /// # Errors
    ///
    /// Returns [`FoliationError::StaleBookkeeping`] if stored foliation
    /// bookkeeping belongs to an older geometry revision. Returns the relevant
    /// [`FoliationError`] variant if live vertex labels are missing, out of
    /// range, inconsistent with stored slice sizes, violate open-boundary
    /// interval/order invariants, or violate toroidal spacelike-ring invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     tri.validate_foliation()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_foliation(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };
        if !self.has_current_foliation() {
            return Err(self.stale_foliation_error());
        }

        let vertex_count = self.geometry.vertex_count();
        if foliation.labeled_vertex_count() != vertex_count {
            return Err(FoliationError::LabelCountMismatch {
                labeled: foliation.labeled_vertex_count(),
                expected: vertex_count,
            }
            .into());
        }

        for (t, &size) in foliation.slice_sizes().iter().enumerate() {
            if size == 0 {
                return Err(FoliationError::EmptySlice { slice: t }.into());
            }
        }

        let mut live_slice_sizes = vec![0usize; foliation.slice_sizes().len()];

        for (vertex, vh) in self.geometry.vertices().enumerate() {
            let Some(label) = self.geometry.vertex_data_by_key(vh.vertex_key()) else {
                return Err(FoliationError::MissingVertexLabel { vertex }.into());
            };

            let slice = label as usize;
            if slice >= live_slice_sizes.len() {
                return Err(FoliationError::OutOfRangeVertexLabel {
                    vertex,
                    label,
                    expected_range_end: live_slice_sizes.len(),
                }
                .into());
            }

            live_slice_sizes[slice] += 1;
        }

        for (slice, (&expected, &actual)) in foliation
            .slice_sizes()
            .iter()
            .zip(live_slice_sizes.iter())
            .enumerate()
        {
            if expected != actual {
                return Err(FoliationError::LabelMismatch {
                    slice,
                    expected,
                    actual,
                }
                .into());
            }
        }

        match self.metadata.topology {
            CdtTopology::OpenBoundary => {
                self.validate_open_boundary_spatial_intervals()?;
            }
            CdtTopology::Toroidal => {
                self.validate_toroidal_spatial_rings()?;
                self.validate_toroidal_temporal_wraparound()?;
            }
        }

        Ok(())
    }

    /// Validates that the time direction wraps for toroidal topology.
    fn validate_toroidal_temporal_wraparound(&self) -> CdtResult<()> {
        let total = self.metadata.time_slices.get();
        if total < 2 {
            return Ok(());
        }
        let Ok(num_slices) = usize::try_from(total) else {
            return Err(CdtError::InvalidTriangulationMetadata {
                field: TriangulationMetadataField::Timeslices,
                topology: self.metadata.topology,
                provided_value: total.to_string(),
                expected: "representable as usize".to_string(),
            });
        };

        let mut neighbor_slices: Vec<HashSet<usize>> = vec![HashSet::new(); num_slices];
        for edge in self.geometry.edges() {
            let Ok((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let Some(t0) = self.geometry.vertex_data_by_key(v0.vertex_key()) else {
                continue;
            };
            let Some(t1) = self.geometry.vertex_data_by_key(v1.vertex_key()) else {
                continue;
            };
            if t0 >= total || t1 >= total {
                continue;
            }
            if self.time_step_distance(t0, t1) != 1 {
                continue;
            }
            let s0 = t0 as usize;
            let s1 = t1 as usize;
            neighbor_slices[s0].insert(s1);
            neighbor_slices[s1].insert(s0);
        }

        for (slice, neighbors) in neighbor_slices.iter().enumerate() {
            let prev = if slice == 0 {
                num_slices - 1
            } else {
                slice - 1
            };
            let next = (slice + 1) % num_slices;
            if !neighbors.contains(&prev) {
                return Err(FoliationError::MissingTemporalWrapAround {
                    slice,
                    missing_neighbor: prev,
                }
                .into());
            }
            if !neighbors.contains(&next) {
                return Err(FoliationError::MissingTemporalWrapAround {
                    slice,
                    missing_neighbor: next,
                }
                .into());
            }
        }

        Ok(())
    }

    /// Collects the spacelike subgraph for each live time slice.
    ///
    /// Both open-boundary interval validation and toroidal ring validation use
    /// this shared adjacency view so they enforce the same label and edge
    /// interpretation before applying topology-specific shape checks.
    fn spacelike_adjacency_by_slice(
        &self,
        num_slices: usize,
    ) -> Vec<HashMap<DelaunayVertexHandle, Vec<DelaunayVertexHandle>>> {
        let mut spacelike_neighbors: Vec<HashMap<DelaunayVertexHandle, Vec<DelaunayVertexHandle>>> =
            vec![HashMap::new(); num_slices];

        for vertex in self.geometry.vertices() {
            let Some(label) = self.geometry.vertex_data_by_key(vertex.vertex_key()) else {
                continue;
            };
            let slice = label as usize;
            if slice < num_slices {
                spacelike_neighbors[slice].entry(vertex).or_default();
            }
        }

        for edge in self.geometry.edges() {
            let Ok((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let Some(t0) = self.geometry.vertex_data_by_key(v0.vertex_key()) else {
                continue;
            };
            let Some(t1) = self.geometry.vertex_data_by_key(v1.vertex_key()) else {
                continue;
            };
            if t0 != t1 {
                continue;
            }
            let slice = t0 as usize;
            if slice >= num_slices {
                continue;
            }
            spacelike_neighbors[slice]
                .entry(v0.clone())
                .or_default()
                .push(v1.clone());
            spacelike_neighbors[slice].entry(v1).or_default().push(v0);
        }

        spacelike_neighbors
    }

    /// Validates that every open-boundary spatial slice forms one interval.
    ///
    /// This topology-specific pass underpins [`Self::validate_foliation`]. It
    /// checks spacelike degrees, path connectivity, coordinate order, and
    /// adjacent-slab crossings so a successful public validation proves that
    /// each stored slice is one consistently embedded open interval.
    #[expect(
        clippy::too_many_lines,
        reason = "open-boundary interval validation keeps the slice-degree, path-order, coordinate-order, and slab-crossing diagnostics in one invariant pass"
    )]
    fn validate_open_boundary_spatial_intervals(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };

        let num_slices = foliation.slice_sizes().len();
        let spacelike_neighbors = self.spacelike_adjacency_by_slice(num_slices);
        let mut slice_orders = Vec::with_capacity(num_slices);

        for (slice, adjacency) in spacelike_neighbors.into_iter().enumerate() {
            let expected_size = foliation.slice_sizes()[slice];
            if adjacency.len() != expected_size {
                return Err(FoliationError::SpacelikeSubgraphSizeMismatch {
                    slice,
                    observed: adjacency.len(),
                    expected: expected_size,
                }
                .into());
            }

            if expected_size == 1 {
                if let Some((vertex, neighbors)) = adjacency
                    .iter()
                    .find(|(_, neighbors)| !neighbors.is_empty())
                {
                    return Err(FoliationError::SpacelikeOpenSliceDegreeViolation {
                        slice,
                        vertex: format!("{:?}", vertex.vertex_key()),
                        observed_degree: neighbors.len(),
                    }
                    .into());
                }
                let Some(single_vertex) = adjacency.keys().next().cloned() else {
                    return Err(FoliationError::SpacelikeSubgraphSizeMismatch {
                        slice,
                        observed: 0,
                        expected: expected_size,
                    }
                    .into());
                };
                let coordinate_order = self.open_boundary_coordinate_order(&[single_vertex])?;
                slice_orders.push(coordinate_order);
                continue;
            }

            let mut endpoints = Vec::with_capacity(2);
            for (vertex, neighbors) in &adjacency {
                match neighbors.len() {
                    1 => endpoints.push(vertex.clone()),
                    2 => {}
                    observed_degree => {
                        return Err(FoliationError::SpacelikeOpenSliceDegreeViolation {
                            slice,
                            vertex: format!("{:?}", vertex.vertex_key()),
                            observed_degree,
                        }
                        .into());
                    }
                }
            }

            let [start, target] = endpoints.as_slice() else {
                return Err(FoliationError::SpacelikeOpenSliceEndpointCount {
                    slice,
                    observed: endpoints.len(),
                    expected: 2,
                }
                .into());
            };

            let target = target.clone();
            let mut visited: HashSet<DelaunayVertexHandle> = HashSet::new();
            let mut ordered_vertices = Vec::with_capacity(expected_size);
            let mut previous: Option<DelaunayVertexHandle> = None;
            let mut current = start.clone();

            loop {
                if !visited.insert(current.clone()) {
                    return Err(FoliationError::SpacelikeNonOpenInterval {
                        slice,
                        walked: visited.len(),
                        expected: expected_size,
                    }
                    .into());
                }
                ordered_vertices.push(current.clone());

                let Some(neighbors) = adjacency.get(&current) else {
                    return Err(FoliationError::SpacelikeNonOpenInterval {
                        slice,
                        walked: visited.len(),
                        expected: expected_size,
                    }
                    .into());
                };
                let Some(next) = neighbors
                    .iter()
                    .find(|neighbor| previous.as_ref() != Some(*neighbor))
                    .cloned()
                else {
                    break;
                };
                previous = Some(current);
                current = next;
            }

            if current != target || visited.len() != expected_size {
                return Err(FoliationError::SpacelikeNonOpenInterval {
                    slice,
                    walked: visited.len(),
                    expected: expected_size,
                }
                .into());
            }
            let coordinate_order =
                self.open_boundary_coordinate_order(ordered_vertices.as_slice())?;
            if !orders_match_with_orientation(ordered_vertices.as_slice(), &coordinate_order) {
                return Err(FoliationError::OpenBoundarySpatialOrderMismatch {
                    slice,
                    path_vertices: ordered_vertices.len(),
                    coordinate_vertices: coordinate_order.len(),
                }
                .into());
            }
            slice_orders.push(coordinate_order);
        }

        self.validate_open_boundary_slab_embedding(&slice_orders)?;

        Ok(())
    }

    /// Returns the backend x-coordinate order for one open-boundary slice.
    ///
    /// Open strips use this to ensure the combinatorial spacelike interval and
    /// displayed spatial embedding describe the same spatial ordering. Evolved
    /// CDT moves are still abstract bistellar edits, so exact coordinate
    /// re-embedding is tracked separately from this combinatorial validator.
    fn open_boundary_coordinate_order(
        &self,
        vertices: &[DelaunayVertexHandle],
    ) -> CdtResult<Vec<DelaunayVertexHandle>> {
        let mut keyed_vertices = Vec::with_capacity(vertices.len());
        for vertex in vertices {
            let raw_coordinates = self.geometry.vertex_coordinates(vertex).map_err(|err| {
                CdtError::ValidationFailed {
                    check: CdtValidationCheck::Geometry,
                    failure: CdtValidationFailure::VertexCoordinateReadFailed {
                        vertex: format!("{:?}", vertex.vertex_key()),
                        detail: err.to_string(),
                    },
                }
            })?;
            let coordinate = SpacetimeCoordinate::try_from_space_time_slice(raw_coordinates)
                .map_err(|err| CdtError::ValidationFailed {
                    check: CdtValidationCheck::Geometry,
                    failure: CdtValidationFailure::from_spacetime_coordinate_error(
                        format!("{:?}", vertex.vertex_key()),
                        err,
                    ),
                })?;
            let Some(label) = self.geometry.vertex_data_by_key(vertex.vertex_key()) else {
                return Err(CdtError::ValidationFailed {
                    check: CdtValidationCheck::FoliationAssignment,
                    failure: CdtValidationFailure::MissingVertexTimeLabel {
                        vertex: format!("{:?}", vertex.vertex_key()),
                    },
                });
            };
            if (coordinate.time() - f64::from(label)).abs() > OPEN_BOUNDARY_TIME_COORDINATE_EPSILON
            {
                return Err(FoliationError::OpenBoundaryTimeCoordinateMismatch {
                    label,
                    vertex: format!("{:?}", vertex.vertex_key()),
                    y: coordinate.time().to_string(),
                }
                .into());
            }
            keyed_vertices.push((
                vertex.clone(),
                coordinate.space(),
                coordinate.time(),
                format!("{:?}", vertex.vertex_key()),
            ));
        }

        keyed_vertices.sort_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| left.2.total_cmp(&right.2))
                .then_with(|| left.3.cmp(&right.3))
        });
        Ok(keyed_vertices
            .into_iter()
            .map(|(vertex, _, _, _)| vertex)
            .collect())
    }

    /// Validates that adjacent open-boundary slices form noncrossing slabs.
    fn validate_open_boundary_slab_embedding(
        &self,
        slice_orders: &[Vec<DelaunayVertexHandle>],
    ) -> CdtResult<()> {
        if slice_orders.len() < 2 {
            return Ok(());
        }

        let mut slab_edges: Vec<Vec<SlabEdge>> = vec![Vec::new(); slice_orders.len() - 1];
        for edge in self.geometry.edges() {
            let Ok((v0, v1)) = self.geometry.edge_endpoints(&edge) else {
                continue;
            };
            let Some(t0) = self.geometry.vertex_data_by_key(v0.vertex_key()) else {
                continue;
            };
            let Some(t1) = self.geometry.vertex_data_by_key(v1.vertex_key()) else {
                continue;
            };
            if t0.abs_diff(t1) != 1 {
                continue;
            }
            let lower_slice = t0.min(t1) as usize;
            if lower_slice >= slab_edges.len() {
                continue;
            }
            let (lower, upper) = if t0 < t1 { (v0, v1) } else { (v1, v0) };
            slab_edges[lower_slice].push((lower, upper));
        }

        let slice_positions: Vec<HashMap<DelaunayVertexHandle, usize>> = slice_orders
            .iter()
            .map(|order| {
                order
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(position, vertex)| (vertex, position))
                    .collect()
            })
            .collect();

        for (lower_slice, edges) in slab_edges.iter().enumerate() {
            if let Some((first, second)) = first_slab_crossing(
                edges,
                &slice_positions[lower_slice],
                &slice_positions[lower_slice + 1],
            ) {
                return Err(FoliationError::OpenBoundarySlabEdgeCrossing {
                    lower_slice,
                    first_edge: open_slab_edge_label(first),
                    second_edge: open_slab_edge_label(second),
                }
                .into());
            }
        }

        Ok(())
    }

    /// Validates that every spatial slice forms a closed S¹.
    fn validate_toroidal_spatial_rings(&self) -> CdtResult<()> {
        let Some(foliation) = &self.foliation else {
            return Ok(());
        };

        let num_slices = foliation.slice_sizes().len();
        let spacelike_neighbors = self.spacelike_adjacency_by_slice(num_slices);

        for (slice, adjacency) in spacelike_neighbors.iter().enumerate() {
            let expected_size = foliation.slice_sizes()[slice];
            if adjacency.len() != expected_size {
                return Err(FoliationError::SpacelikeSubgraphSizeMismatch {
                    slice,
                    observed: adjacency.len(),
                    expected: expected_size,
                }
                .into());
            }
            for (vertex, neighbors) in adjacency {
                if neighbors.len() != 2 {
                    return Err(FoliationError::SpacelikeDegreeViolation {
                        slice,
                        vertex: format!("{:?}", vertex.vertex_key()),
                        observed_degree: neighbors.len(),
                    }
                    .into());
                }
            }

            let Some(start) = adjacency.keys().next() else {
                continue;
            };
            let mut visited: HashSet<DelaunayVertexHandle> = HashSet::new();
            visited.insert(start.clone());
            let mut prev = start.clone();
            let mut current = adjacency[start][0].clone();
            while current != *start {
                if !visited.insert(current.clone()) {
                    return Err(FoliationError::SpacelikeNonClosedRing {
                        slice,
                        walked: visited.len(),
                        expected: expected_size,
                    }
                    .into());
                }
                let neighbors = &adjacency[&current];
                let next = if neighbors[0] == prev {
                    neighbors[1].clone()
                } else {
                    neighbors[0].clone()
                };
                prev = current;
                current = next;
            }
            if visited.len() != expected_size {
                return Err(FoliationError::SpacelikeNonClosedRing {
                    slice,
                    walked: visited.len(),
                    expected: expected_size,
                }
                .into());
            }
        }

        Ok(())
    }

    /// Assign a foliation to an existing triangulation by binning vertices
    /// by their y-coordinate into `num_slices` equal bands.
    ///
    /// The y-coordinate range is determined from the actual vertex coordinates.
    /// Band `t` covers `[y_min + t * band_height, y_min + (t+1) * band_height)`.
    /// Time labels are written directly to vertex data.
    ///
    /// This is approximate — useful for testing but not guaranteed to produce
    /// a valid causal structure.
    ///
    /// # Errors
    ///
    /// Returns error if vertex coordinates cannot be read, if y-bucket
    /// assignment would leave any time slice empty, if the requested slice count
    /// violates the triangulation topology, or if writing vertex labels or
    /// clearing stale simplex labels in the backend fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    /// use std::num::NonZeroU32;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let mut tri = CdtTriangulation::from_seeded_points(12, 3, 2, 42)?;
    ///     let Some(num_slices) = NonZeroU32::new(3) else {
    ///         return Ok(());
    ///     };
    ///     tri.assign_foliation_by_y(num_slices)?;
    ///
    ///     assert!(tri.has_foliation());
    ///     assert_eq!(tri.slice_sizes().iter().sum::<usize>(), tri.vertex_count());
    ///     Ok(())
    /// }
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "foliation assignment stages labels, writes backend payloads, and rolls back on failure to preserve atomic metadata/foliation invariants"
    )]
    pub fn assign_foliation_by_y(&mut self, num_slices: NonZeroU32) -> CdtResult<()> {
        let raw_num_slices = num_slices.get();
        let y_coords: Vec<(DelaunayVertexHandle, f64)> = self
            .geometry
            .vertices()
            .map(|vh| {
                let coords = self.geometry.vertex_coordinates(&vh).map_err(|e| {
                    CdtError::ValidationFailed {
                        check: CdtValidationCheck::FoliationAssignment,
                        failure: CdtValidationFailure::VertexCoordinateReadFailed {
                            vertex: format!("{:?}", vh.vertex_key()),
                            detail: e.to_string(),
                        },
                    }
                })?;
                if coords.len() < 2 {
                    return Err(CdtError::ValidationFailed {
                        check: CdtValidationCheck::FoliationAssignment,
                        failure: CdtValidationFailure::VertexCoordinateDimension {
                            vertex: format!("{:?}", vh.vertex_key()),
                            actual: coords.len(),
                            expected_minimum: 2,
                        },
                    });
                }
                Ok((vh, coords[1]))
            })
            .collect::<CdtResult<Vec<_>>>()?;

        let y_min = y_coords
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        let y_max = y_coords
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::NEG_INFINITY, f64::max);

        let range = y_max - y_min;
        let band_height = if range.abs() < f64::EPSILON {
            1.0
        } else {
            range / f64::from(raw_num_slices)
        };

        let mut assignments = Vec::with_capacity(y_coords.len());
        let mut slice_sizes = vec![0usize; raw_num_slices as usize];
        for (vh, y) in &y_coords {
            let t = if range.abs() < f64::EPSILON {
                0
            } else {
                let band_index = ((y - y_min) / band_height).floor();
                f64_band_to_u32(band_index, raw_num_slices - 1)
            };
            assignments.push((vh.vertex_key(), t));
            slice_sizes[t as usize] += 1;
        }

        Self::check_time_slices(self.metadata.topology, raw_num_slices)?;

        let foliation =
            Foliation::from_slice_sizes(slice_sizes, num_slices).map_err(CdtError::from)?;

        let face_keys: Vec<_> = self.geometry.faces().map(|f| f.simplex_key()).collect();
        let previous_simplex_data: Vec<_> = face_keys
            .iter()
            .map(|&key| (key, self.geometry.simplex_data_by_key(key)))
            .collect();
        let previous_vertex_data: Vec<_> = assignments
            .iter()
            .map(|&(key, _)| (key, self.geometry.vertex_data_by_key(key)))
            .collect();

        let rollback_payloads = |geometry: &mut DelaunayBackend2D| {
            let mut rollback_failures = Vec::new();

            for &(key, data) in &previous_simplex_data {
                if let Err(err) = geometry.set_simplex_data_by_key(key, data) {
                    rollback_failures.push(BackendRollbackFailure {
                        operation: BackendMutationOperation::SetSimplexDataByKey,
                        target: format!("face {key:?}"),
                        detail: err.to_string(),
                    });
                }
            }

            for &(key, data) in &previous_vertex_data {
                if let Err(err) = geometry.set_vertex_data_by_key(key, data) {
                    rollback_failures.push(BackendRollbackFailure {
                        operation: BackendMutationOperation::SetVertexDataByKey,
                        target: format!("vertex {key:?}"),
                        detail: err.to_string(),
                    });
                }
            }

            BackendRollbackFailures::new(rollback_failures)
        };

        for &key in &face_keys {
            if let Err(err) = self.geometry.set_simplex_data_by_key(key, None) {
                let operation = BackendMutationOperation::SetSimplexDataByKey;
                let target = format!("face {key:?}");
                let detail = err.to_string();
                let rollback_failures = rollback_payloads(&mut self.geometry);
                return if rollback_failures.is_empty() {
                    Err(CdtError::BackendMutationFailed {
                        operation,
                        target,
                        detail,
                    })
                } else {
                    Err(CdtError::BackendRollbackFailed {
                        operation,
                        target,
                        detail,
                        rollback_failures,
                    })
                };
            }
        }

        for (vertex_key, t) in assignments {
            if let Err(err) = self.geometry.set_vertex_data_by_key(vertex_key, Some(t)) {
                let operation = BackendMutationOperation::SetVertexDataByKey;
                let target = format!("vertex {vertex_key:?}");
                let detail = format!("failed while assigning time label {t}: {err}");
                let rollback_failures = rollback_payloads(&mut self.geometry);
                return if rollback_failures.is_empty() {
                    Err(CdtError::BackendMutationFailed {
                        operation,
                        target,
                        detail,
                    })
                } else {
                    Err(CdtError::BackendRollbackFailed {
                        operation,
                        target,
                        detail,
                        rollback_failures,
                    })
                };
            }
        }

        self.metadata.time_slices = num_slices;
        self.bump_modification_count();
        self.foliation = Some(foliation);
        self.mark_foliation_synchronized();
        Ok(())
    }

    /// Returns `true` if this triangulation has current foliation bookkeeping.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert!(tri.has_foliation());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn has_foliation(&self) -> bool {
        self.has_current_foliation()
    }

    /// Returns a reference to the foliation, if present.
    ///
    /// A triangulation can still contain backend vertex labels while stored
    /// foliation bookkeeping is stale; this method returns `None` in that case.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert_eq!(tri.foliation().map(|foliation| foliation.num_slices().get()), Some(2));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn foliation(&self) -> Option<&Foliation> {
        if self.has_current_foliation() {
            self.foliation.as_ref()
        } else {
            None
        }
    }

    /// Returns the time slice label for a vertex, or `None` if no current
    /// foliation is present, the stored foliation is stale, or the vertex is
    /// unlabeled.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] when `vertex` is foreign, stale,
    /// or absent from the current backend topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert!(tri.geometry().vertices().all(|vertex| {
    ///         tri.time_label(&vertex).is_ok_and(|label| label.is_some())
    ///     }));
    ///     Ok(())
    /// }
    /// ```
    pub fn time_label(&self, vertex: &DelaunayVertexHandle) -> CdtResult<Option<u32>> {
        let label = self
            .geometry
            .vertex_data(vertex)
            .map_err(|error| geometry_query_error("time label", &error))?;
        Ok(self.foliation().and(label))
    }

    /// Iterates over all vertex handles that belong to time slice `t`.
    ///
    /// Returns an empty iterator when no current foliation exists. That includes
    /// the stale-bookkeeping case after geometry mutation but before foliation
    /// resynchronization.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     assert_eq!(tri.vertices_at_time(0).count(), 4);
    ///     assert!(tri.vertices_at_time(99).next().is_none());
    ///     Ok(())
    /// }
    /// ```
    pub fn vertices_at_time(&self, t: u32) -> impl Iterator<Item = DelaunayVertexHandle> + '_ {
        let has_current_foliation = self.has_current_foliation();
        self.geometry.vertices().filter(move |vertex| {
            has_current_foliation
                && self
                    .geometry
                    .vertex_data(vertex)
                    .is_ok_and(|label| label == Some(t))
        })
    }

    /// Returns per-slice vertex counts, or an empty slice if no foliation.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     assert_eq!(tri.slice_sizes(), &[4, 4, 4]);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub fn slice_sizes(&self) -> &[usize] {
        self.foliation().map_or(&[], Foliation::slice_sizes)
    }

    /// Counts strict CDT triangles by the time slab of their lower slice.
    ///
    /// The returned vector has one entry per time slice. For open-boundary
    /// strips, the final slice usually has zero lower-slab triangles because no
    /// future slice exists; toroidal triangulations wrap the last slab back to
    /// slice zero.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::MeasurementCountOverflow`] if any per-slice triangle
    /// count exceeds the compact `u32` storage used by measurements and scalar
    /// traces.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 3)?;
    ///     assert_eq!(tri.volume_profile()?, vec![6, 6, 0]);
    ///     Ok(())
    /// }
    /// ```
    pub fn volume_profile(&self) -> CdtResult<Vec<u32>> {
        if !self.has_current_foliation() {
            return Ok(Vec::new());
        }

        let Ok(slice_count) = usize::try_from(self.metadata.time_slices.get()) else {
            return Ok(Vec::new());
        };
        let mut profile = vec![0_u32; slice_count];

        for face in self.geometry.faces() {
            let Some(slice) = self.face_time_slice(&face) else {
                continue;
            };
            let Ok(index) = usize::try_from(slice) else {
                continue;
            };
            if let Some(count) = profile.get_mut(index) {
                *count = count
                    .checked_add(1)
                    .ok_or_else(volume_profile_count_overflow)?;
            }
        }

        Ok(profile)
    }

    /// Computes the temporal step distance between two time labels.
    #[must_use]
    pub(super) fn time_step_distance(&self, t0: u32, t1: u32) -> u32 {
        let raw = t0.abs_diff(t1);
        if matches!(self.metadata.topology, CdtTopology::Toroidal) {
            let total = self.metadata.time_slices.get();
            if t0 < total && t1 < total {
                return raw.min(total - raw);
            }
        }
        raw
    }

    /// Topology-aware variant of [`crate::cdt::foliation::classify_simplex`].
    fn classify_simplex_with_topology(&self, t0: u32, t1: u32, t2: u32) -> Option<SimplexType> {
        let mut dists = [
            self.time_step_distance(t0, t1),
            self.time_step_distance(t1, t2),
            self.time_step_distance(t0, t2),
        ];
        dists.sort_unstable();
        if dists != [0, 1, 1] {
            return None;
        }

        let (base_slice, apex_slice) = if t0 == t1 {
            (t0, t2)
        } else if t1 == t2 {
            (t1, t0)
        } else if t0 == t2 {
            (t0, t1)
        } else {
            return None;
        };

        let total = self.metadata.time_slices.get();
        let toroidal = matches!(self.metadata.topology, CdtTopology::Toroidal);
        let up_apex = if toroidal {
            (base_slice + 1) % total
        } else {
            base_slice.checked_add(1)?
        };
        let down_apex = if toroidal {
            if base_slice == 0 {
                total - 1
            } else {
                base_slice - 1
            }
        } else {
            base_slice.checked_sub(1)?
        };
        if apex_slice == up_apex {
            Some(SimplexType::Up)
        } else if apex_slice == down_apex {
            Some(SimplexType::Down)
        } else {
            None
        }
    }

    /// Returns the lower time-slab index assigned to a classifiable CDT face.
    fn face_time_slice(&self, face: &DelaunayFaceHandle) -> Option<u32> {
        self.simplex_type(face).ok()??;

        let vertices = self.geometry.face_vertices(face).ok()?;
        let [v0, v1, v2] = exactly_three(vertices)?;

        let labels = [
            self.geometry.vertex_data_by_key(v0.vertex_key())?,
            self.geometry.vertex_data_by_key(v1.vertex_key())?,
            self.geometry.vertex_data_by_key(v2.vertex_key())?,
        ];

        match self.metadata.topology {
            CdtTopology::OpenBoundary => Some(labels[0].min(labels[1]).min(labels[2])),
            CdtTopology::Toroidal => {
                let total = self.metadata.time_slices.get();

                let first = labels[0];
                let mut second = None;
                for &label in &labels[1..] {
                    if label != first {
                        if second.is_some_and(|distinct| distinct != label) {
                            return None;
                        }
                        second = Some(label);
                    }
                }
                let second = second?;

                let next_slice = |slice: u32| slice.checked_add(1).map(|next| next % total);
                if next_slice(first) == Some(second) {
                    Some(first)
                } else if next_slice(second) == Some(first) {
                    Some(second)
                } else {
                    None
                }
            }
        }
    }

    /// Returns the causal classification of an edge from endpoint time labels.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] when `edge` is foreign, stale, or
    /// absent, or when either live endpoint payload cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert!(tri.geometry().edges().any(|edge| {
    ///         tri.edge_type(&edge).is_ok_and(|edge_type| edge_type.is_some())
    ///     }));
    ///     Ok(())
    /// }
    /// ```
    pub fn edge_type(&self, edge: &DelaunayEdgeHandle) -> CdtResult<Option<EdgeType>> {
        let (v0, v1) = self
            .geometry
            .edge_endpoints(edge)
            .map_err(|error| geometry_query_error("edge classification", &error))?;
        if self.foliation().is_none() {
            return Ok(None);
        }
        let Some(t0) = self
            .geometry
            .vertex_data(&v0)
            .map_err(|error| geometry_query_error("edge endpoint label", &error))?
        else {
            return Ok(None);
        };
        let Some(t1) = self
            .geometry
            .vertex_data(&v1)
            .map_err(|error| geometry_query_error("edge endpoint label", &error))?
        else {
            return Ok(None);
        };

        Ok(Some(match self.time_step_distance(t0, t1) {
            0 => EdgeType::Spacelike,
            1 => EdgeType::Timelike,
            _ => EdgeType::Acausal,
        }))
    }

    /// Classifies a triangle as Up (2,1) or Down (1,2) from vertex time labels.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] when `face` is foreign, stale, or
    /// absent, or when a live vertex payload cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert!(tri.geometry().faces().all(|face| {
    ///         tri.simplex_type(&face).is_ok_and(|simplex_type| simplex_type.is_some())
    ///     }));
    ///     Ok(())
    /// }
    /// ```
    pub fn simplex_type(&self, face: &DelaunayFaceHandle) -> CdtResult<Option<SimplexType>> {
        let verts = self
            .geometry
            .face_vertices(face)
            .map_err(|error| geometry_query_error("simplex classification", &error))?;
        let Some([v0, v1, v2]) = exactly_three(verts) else {
            return Ok(None);
        };
        if self.foliation().is_none() {
            return Ok(None);
        }
        let labels = [v0, v1, v2].map(|vertex| self.geometry.vertex_data(&vertex));
        let [t0, t1, t2] = labels.map(|label| {
            label.map_err(|error| geometry_query_error("simplex vertex label", &error))
        });
        let (Some(t0), Some(t1), Some(t2)) = (t0?, t1?, t2?) else {
            return Ok(None);
        };
        Ok(match self.metadata.topology {
            CdtTopology::Toroidal => self.classify_simplex_with_topology(t0, t1, t2),
            CdtTopology::OpenBoundary => classify_simplex(Some(t0), Some(t1), Some(t2)),
        })
    }

    /// Reads the stored simplex type from simplex data, if previously classified.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] when `face` is foreign, stale, or
    /// absent from the current backend topology.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert!(tri.geometry().faces().all(|face| {
    ///         tri.simplex_type_from_data(&face).is_ok_and(|simplex_type| simplex_type.is_some())
    ///     }));
    ///     Ok(())
    /// }
    /// ```
    pub fn simplex_type_from_data(
        &self,
        face: &DelaunayFaceHandle,
    ) -> CdtResult<Option<SimplexType>> {
        let raw = self
            .geometry
            .simplex_data(face)
            .map_err(|error| geometry_query_error("stored simplex classification", &error))?;
        Ok(self
            .foliation()
            .and_then(|_| raw.and_then(SimplexType::from_i32)))
    }

    /// Returns the edge classification for a triangular face.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::ValidationFailed`] when `face` is foreign, stale, or
    /// absent, or when a live vertex payload cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert!(tri.geometry().faces().all(|face| {
    ///         tri.face_edge_types(&face).is_ok_and(|edges| edges.is_some_and(|edges| {
    ///             edges.iter().filter(|&&edge| edge == EdgeType::Spacelike).count() == 1
    ///                 && edges.iter().filter(|&&edge| edge == EdgeType::Timelike).count() == 2
    ///         }))
    ///     }));
    ///     Ok(())
    /// }
    /// ```
    pub fn face_edge_types(&self, face: &DelaunayFaceHandle) -> CdtResult<Option<[EdgeType; 3]>> {
        let verts = self
            .geometry
            .face_vertices(face)
            .map_err(|error| geometry_query_error("face edge classification", &error))?;
        let Some([v0, v1, v2]) = exactly_three(verts) else {
            return Ok(None);
        };
        if self.foliation().is_none() {
            return Ok(None);
        }

        let labels = [v0, v1, v2].map(|vertex| self.geometry.vertex_data(&vertex));
        let [t0, t1, t2] = labels
            .map(|label| label.map_err(|error| geometry_query_error("face vertex label", &error)));
        let (Some(t0), Some(t1), Some(t2)) = (t0?, t1?, t2?) else {
            return Ok(None);
        };
        let t = [t0, t1, t2];

        let edge_classify = |a: u32, b: u32| -> EdgeType {
            match self.time_step_distance(a, b) {
                0 => EdgeType::Spacelike,
                1 => EdgeType::Timelike,
                _ => EdgeType::Acausal,
            }
        };

        Ok(Some([
            edge_classify(t[0], t[1]),
            edge_classify(t[1], t[2]),
            edge_classify(t[2], t[0]),
        ]))
    }

    /// Validates that every finite face has a strict CDT simplex classification.
    ///
    /// # Errors
    ///
    /// Returns [`FoliationError::StaleBookkeeping`] if stored foliation
    /// bookkeeping belongs to an older geometry revision. Returns
    /// [`CdtError::ValidationFailed`] if any face in a current foliated
    /// triangulation cannot be classified as a strict Up or Down CDT simplex.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     tri.validate_simplex_classification()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn validate_simplex_classification(&self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return Ok(());
        }
        if !self.has_current_foliation() {
            return Err(self.stale_foliation_error());
        }

        for face in self.geometry.faces() {
            if self.simplex_type(&face)?.is_none() {
                return Err(CdtError::ValidationFailed {
                    check: CdtValidationCheck::SimplexClassification,
                    failure: CdtValidationFailure::NonStrictSimplex {
                        face: format!("{:?}", face.simplex_key()),
                    },
                });
            }
        }

        Ok(())
    }

    /// Counts top-dimensional simplices that are not strict causal CDT simplices.
    ///
    /// In the current 1+1 implementation, the top-dimensional simplices are
    /// triangle faces, and a strict causal CDT triangle must classify as
    /// [`SimplexType::Up`] `(2,1)` or [`SimplexType::Down`] `(1,2)`. Purely
    /// spacelike faces, purely
    /// timelike/non-spacelike faces, multi-slice faces, missing-label faces,
    /// and malformed faces all contribute to this count. A valid foliated CDT
    /// initial state has count zero.
    ///
    /// # Errors
    ///
    /// Returns [`FoliationError::MissingBookkeeping`] if the triangulation has
    /// no stored foliation to classify against.
    ///
    /// Returns [`FoliationError::StaleBookkeeping`] if stored foliation
    /// bookkeeping belongs to an older geometry revision.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert_eq!(tri.strict_causal_simplex_violation_count()?, 0);
    ///     Ok(())
    /// }
    /// ```
    pub fn strict_causal_simplex_violation_count(&self) -> CdtResult<usize> {
        if self.foliation.is_none() {
            return Err(FoliationError::MissingBookkeeping.into());
        }
        if !self.has_current_foliation() {
            return Err(self.stale_foliation_error());
        }

        self.geometry.faces().try_fold(0_usize, |count, face| {
            self.simplex_type(&face)
                .map(|simplex_type| count + usize::from(simplex_type.is_none()))
        })
    }

    /// Classifies every triangle and stores the result as simplex data.
    ///
    /// # Errors
    ///
    /// Returns [`FoliationError::StaleBookkeeping`] if stored foliation
    /// bookkeeping belongs to an older geometry revision. Returns
    /// [`CdtError::ValidationFailed`] if a current foliated face is not an Up or
    /// Down CDT simplex. Returns [`CdtError::BackendMutationFailed`] if writing simplex
    /// payloads fails, or [`CdtError::BackendRollbackFailed`] if restoring
    /// previous payloads also fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::triangulation::*;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let mut tri = CdtTriangulation::from_cdt_strip(4, 2)?;
    ///     assert_eq!(tri.classify_all_simplices()?, Some(tri.face_count()));
    ///     Ok(())
    /// }
    /// ```
    pub fn classify_all_simplices(&mut self) -> CdtResult<Option<usize>> {
        if self.foliation.is_none() {
            return Ok(None);
        }
        if !self.has_current_foliation() {
            return Err(self.stale_foliation_error());
        }

        let faces: Vec<_> = self.geometry.faces().collect();
        let mut classifications = Vec::with_capacity(faces.len());
        for face in &faces {
            let Some(ct) = self.simplex_type(face)? else {
                return Err(CdtError::ValidationFailed {
                    check: CdtValidationCheck::SimplexClassification,
                    failure: CdtValidationFailure::NonStrictSimplex {
                        face: format!("{:?}", face.simplex_key()),
                    },
                });
            };
            classifications.push((face.simplex_key(), ct));
        }

        let count = classifications.len();
        let previous_simplex_data: Vec<_> = faces
            .iter()
            .map(|face| {
                let key = face.simplex_key();
                (key, self.geometry.simplex_data_by_key(key))
            })
            .collect();
        let rollback_simplex_payloads = |geometry: &mut DelaunayBackend2D| {
            let mut rollback_failures = Vec::new();

            for &(key, data) in &previous_simplex_data {
                if let Err(err) = geometry.set_simplex_data_by_key(key, data) {
                    rollback_failures.push(BackendRollbackFailure {
                        operation: BackendMutationOperation::SetSimplexDataByKey,
                        target: format!("face {key:?}"),
                        detail: err.to_string(),
                    });
                }
            }

            BackendRollbackFailures::new(rollback_failures)
        };

        for face in &faces {
            let key = face.simplex_key();
            if let Err(err) = self.geometry.set_simplex_data_by_key(key, None) {
                let operation = BackendMutationOperation::SetSimplexDataByKey;
                let target = format!("face {key:?}");
                let detail = format!(
                    "failed to clear existing simplex payload before classification: {err}"
                );
                let rollback_failures = rollback_simplex_payloads(&mut self.geometry);
                return if rollback_failures.is_empty() {
                    Err(CdtError::BackendMutationFailed {
                        operation,
                        target,
                        detail,
                    })
                } else {
                    Err(CdtError::BackendRollbackFailed {
                        operation,
                        target,
                        detail,
                        rollback_failures,
                    })
                };
            }
        }
        for (key, ct) in classifications {
            if let Err(err) = self
                .geometry
                .set_simplex_data_by_key(key, Some(ct.to_i32()))
            {
                let operation = BackendMutationOperation::SetSimplexDataByKey;
                let target = format!("face {key:?}");
                let detail = format!(
                    "failed to store classified simplex payload {}: {err}",
                    ct.to_i32()
                );
                let rollback_failures = rollback_simplex_payloads(&mut self.geometry);
                return if rollback_failures.is_empty() {
                    Err(CdtError::BackendMutationFailed {
                        operation,
                        target,
                        detail,
                    })
                } else {
                    Err(CdtError::BackendRollbackFailed {
                        operation,
                        target,
                        detail,
                        rollback_failures,
                    })
                };
            }
        }
        Ok(Some(count))
    }

    /// Rebuilds foliation bookkeeping from live backend vertex labels after a topology edit.
    pub(crate) fn synchronize_foliation_from_live_labels(&mut self) -> CdtResult<()> {
        if self.foliation.is_none() {
            return Ok(());
        }

        let slice_sizes = Self::live_slice_sizes_from_vertex_labels(
            &self.geometry,
            self.metadata.time_slices.get(),
        )?;
        let foliation = Foliation::from_slice_sizes(slice_sizes, self.metadata.time_slices)
            .map_err(CdtError::from)?;

        self.foliation = Some(foliation);
        self.mark_foliation_synchronized();
        match self.classify_all_simplices() {
            Ok(_) => Ok(()),
            Err(err) => {
                self.foliation = None;
                self.foliation_synced_at_modification = None;
                Err(err)
            }
        }
    }
}

/// Builds the typed measurement overflow reported by [`CdtTriangulation::volume_profile`].
fn volume_profile_count_overflow() -> CdtError {
    CdtError::MeasurementCountOverflow {
        field: MeasurementCountField::Triangles,
        provided_value: usize::try_from(u32::MAX).map_or(usize::MAX, |max| max.saturating_add(1)),
        max: u32::MAX,
    }
}

const OPEN_BOUNDARY_TIME_COORDINATE_EPSILON: f64 = 1e-9;

fn geometry_query_error(context: &str, error: &DelaunayError) -> CdtError {
    CdtError::ValidationFailed {
        check: CdtValidationCheck::Geometry,
        failure: CdtValidationFailure::BackendGeometry {
            detail: format!("{context} query failed: {error}"),
        },
    }
}

type SlabEdge = (DelaunayVertexHandle, DelaunayVertexHandle);
type SlabCrossing<'a> = (&'a SlabEdge, &'a SlabEdge);

/// Reports whether two slab edges share either endpoint.
fn slab_edges_share_endpoint(first: &SlabEdge, second: &SlabEdge) -> bool {
    let first_endpoints = [&first.0, &first.1];
    let second_endpoints = [&second.0, &second.1];
    first_endpoints
        .iter()
        .any(|endpoint| second_endpoints.contains(endpoint))
}

/// Checks whether two slice orders match up to reversal.
fn orders_match_with_orientation(
    path_order: &[DelaunayVertexHandle],
    coordinate_order: &[DelaunayVertexHandle],
) -> bool {
    path_order == coordinate_order || path_order.iter().eq(coordinate_order.iter().rev())
}

/// Finds the first pair of non-incident timelike slab edges that cross.
///
/// Open-boundary validation calls this while enforcing that adjacent spatial
/// intervals embed as a noncrossing slab. Edges missing position data are
/// ignored because earlier foliation checks own missing-label diagnostics, and
/// incident edges are allowed to meet at shared vertices. The ordered scan keeps
/// the validation path practical for evolved triangulations with many timelike
/// edges while preserving the crossing pair used in public diagnostics.
fn first_slab_crossing<'a>(
    edges: &'a [SlabEdge],
    lower_positions: &HashMap<DelaunayVertexHandle, usize>,
    upper_positions: &HashMap<DelaunayVertexHandle, usize>,
) -> Option<SlabCrossing<'a>> {
    let mut positioned_edges = Vec::with_capacity(edges.len());
    for edge in edges {
        let Some(&lower_index) = lower_positions.get(&edge.0) else {
            continue;
        };
        let Some(&upper_index) = upper_positions.get(&edge.1) else {
            continue;
        };
        positioned_edges.push((lower_index, upper_index, edge));
    }
    positioned_edges.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut seen_by_upper: BTreeMap<usize, Vec<&SlabEdge>> = BTreeMap::new();
    let mut index = 0;
    while index < positioned_edges.len() {
        let lower_index = positioned_edges[index].0;
        let mut group_end = index + 1;
        while group_end < positioned_edges.len() && positioned_edges[group_end].0 == lower_index {
            group_end += 1;
        }

        for &(_, current_upper, current_edge) in &positioned_edges[index..group_end] {
            let Some(first_larger_upper) = current_upper.checked_add(1) else {
                continue;
            };
            let Some((&max_seen_upper, _)) = seen_by_upper.last_key_value() else {
                continue;
            };
            if current_upper >= max_seen_upper {
                continue;
            }
            for (_, earlier_edges) in seen_by_upper.range(first_larger_upper..) {
                if let Some(earlier_edge) = earlier_edges
                    .iter()
                    .copied()
                    .find(|earlier_edge| !slab_edges_share_endpoint(earlier_edge, current_edge))
                {
                    return Some((earlier_edge, current_edge));
                }
            }
        }

        for &(_, upper_index, edge) in &positioned_edges[index..group_end] {
            seen_by_upper.entry(upper_index).or_default().push(edge);
        }
        index = group_end;
    }
    None
}

/// Formats an open-slab edge for topology validation diagnostics.
fn open_slab_edge_label(edge: &SlabEdge) -> String {
    format!("{:?}->{:?}", edge.0.vertex_key(), edge.1.vertex_key())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::TriangulationMetadataField;
    use crate::geometry::generators::build_delaunay2_with_data;
    use std::assert_matches;
    use std::time::Duration;

    const TEST_POINT_SEED: u64 = 0xF011_A710;

    fn slice_count(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("test slice count should be nonzero")
    }

    /// Builds a minimal labeled Delaunay backend for foliation tests.
    fn labeled_triangle_backend(labels: [u32; 3]) -> DelaunayBackend2D {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], labels[0]),
            ([1.0, 0.0], labels[1]),
            ([0.5, 1.0], labels[2]),
        ])
        .expect("Should build labeled triangle");
        DelaunayBackend2D::from_triangulation(dt).expect("test Delaunay triangle should validate")
    }

    /// Builds a Delaunay strip and verifies it is a strict CDT mesh.
    fn strict_strip(
        vertices_per_slice: u32,
        num_slices: u32,
    ) -> CdtTriangulation<DelaunayBackend2D> {
        let tri = CdtTriangulation::from_cdt_strip(vertices_per_slice, num_slices)
            .expect("Delaunay strip construction should succeed");
        assert_eq!(
            tri.vertex_count(),
            vertices_per_slice as usize * num_slices as usize
        );
        assert_eq!(
            tri.face_count(),
            2 * (vertices_per_slice as usize - 1) * (num_slices as usize - 1)
        );
        assert_eq!(
            tri.slice_sizes(),
            vec![vertices_per_slice as usize; num_slices as usize].as_slice()
        );
        tri.validate_foliation()
            .expect("Delaunay strip foliation should validate");
        tri.validate_causality_delaunay()
            .expect("Delaunay strip causality should validate");
        tri.validate_simplex_classification()
            .expect("all Delaunay strip simplices should classify");
        assert_eq!(
            tri.strict_causal_simplex_violation_count()
                .expect("strict strip count should succeed"),
            0
        );
        tri
    }

    #[test]
    fn slice_order_matching_accepts_only_coordinate_order_or_reversal() {
        let tri = strict_strip(4, 2);
        let path_order = tri.vertices_at_time(0).collect::<Vec<_>>();
        let reversed_order = path_order.iter().rev().cloned().collect::<Vec<_>>();
        let mut rotated_order = path_order.clone();
        rotated_order.rotate_left(1);

        assert!(orders_match_with_orientation(&path_order, &path_order));
        assert!(orders_match_with_orientation(&path_order, &reversed_order));
        assert!(
            !orders_match_with_orientation(&path_order, &rotated_order),
            "cyclic rotations are not valid for open-boundary interval slices"
        );
    }

    #[test]
    fn open_boundary_coordinate_order_rejects_mismatched_time_coordinate() {
        let dt = build_delaunay2_with_data(&[
            ([0.0, 0.0], 0),
            ([1.0, 0.0], 0),
            ([0.0, 1.25], 1),
            ([1.0, 1.0], 1),
        ])
        .expect("mismatched-y fixture should build as Delaunay data");
        let backend =
            DelaunayBackend2D::from_triangulation(dt).expect("mismatched-y backend should wrap");

        assert_matches!(
            CdtTriangulation::from_labeled_delaunay(backend, 2, 2),
            Err(CdtError::Foliation(
                FoliationError::OpenBoundaryTimeCoordinateMismatch {
                    label: 1,
                    vertex: _,
                    y,
                }
            )) if y == "1.25"
        );
    }

    #[test]
    fn first_slab_crossing_ignores_incident_edges_and_reports_inversion() {
        let tri = strict_strip(4, 2);
        let lower = tri.vertices_at_time(0).collect::<Vec<_>>();
        let upper = tri.vertices_at_time(1).collect::<Vec<_>>();
        let lower_positions = lower
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, vertex)| (vertex, index))
            .collect::<HashMap<_, _>>();
        let upper_positions = upper
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, vertex)| (vertex, index))
            .collect::<HashMap<_, _>>();
        let incident_edges = [
            (lower[0].clone(), upper[3].clone()),
            (lower[0].clone(), upper[0].clone()),
        ];
        let crossing_edges = [
            (lower[0].clone(), upper[3].clone()),
            (lower[1].clone(), upper[1].clone()),
        ];

        assert!(
            first_slab_crossing(&incident_edges, &lower_positions, &upper_positions).is_none(),
            "edges sharing a vertex are incident and should not be reported as crossings"
        );
        let Some((first, second)) =
            first_slab_crossing(&crossing_edges, &lower_positions, &upper_positions)
        else {
            panic!("expected an inverted non-incident edge pair to be reported");
        };
        assert_eq!(first, &crossing_edges[0]);
        assert_eq!(second, &crossing_edges[1]);
    }

    #[test]
    fn slab_crossing_skips_missing_positions_and_ties() {
        let tri = strict_strip(4, 2);
        let lower = tri.vertices_at_time(0).collect::<Vec<_>>();
        let upper = tri.vertices_at_time(1).collect::<Vec<_>>();
        let mut lower_positions = lower
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, vertex)| (vertex, index))
            .collect::<HashMap<_, _>>();
        let mut upper_positions = upper
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, vertex)| (vertex, index))
            .collect::<HashMap<_, _>>();
        let crossing_edges = [
            (lower[0].clone(), upper[3].clone()),
            (lower[1].clone(), upper[1].clone()),
        ];

        assert!(
            first_slab_crossing(&crossing_edges, &lower_positions, &upper_positions).is_some(),
            "fixture should contain an inverted non-incident pair before position data is removed"
        );

        lower_positions.remove(&lower[1]);
        assert!(
            first_slab_crossing(&crossing_edges, &lower_positions, &upper_positions).is_none(),
            "edges missing lower position data should be skipped rather than reported"
        );

        lower_positions.insert(lower[1].clone(), 0);
        assert!(
            first_slab_crossing(&crossing_edges, &lower_positions, &upper_positions).is_none(),
            "edges with the same lower rank are not ordered crossings"
        );

        lower_positions.insert(lower[1].clone(), 1);
        upper_positions.remove(&upper[3]);
        assert!(
            first_slab_crossing(&crossing_edges, &lower_positions, &upper_positions).is_none(),
            "edges missing upper position data should be skipped rather than reported"
        );
    }

    #[test]
    fn validate_foliation_is_vacuous_without_foliation() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 3, 2, TEST_POINT_SEED)
            .expect("Failed to create triangulation");

        triangulation
            .validate_foliation()
            .expect("missing foliation should validate vacuously");
    }

    #[test]
    fn validate_foliation_rejects_stale_bookkeeping_before_live_labels() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let first_vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex");

        tri.set_vertex_data(&first_vertex, None)
            .expect("Expected valid vertex handle while clearing label");
        assert_matches!(
            tri.validate_foliation(),
            Err(CdtError::Foliation(FoliationError::StaleBookkeeping { .. }))
        );
    }

    #[test]
    fn synchronizing_foliation_detects_missing_and_out_of_range_live_labels() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let first_vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex");

        tri.set_vertex_data(&first_vertex, None)
            .expect("Expected valid vertex handle while clearing label");
        assert_matches!(
            tri.synchronize_foliation_from_live_labels(),
            Err(CdtError::Foliation(FoliationError::MissingVertexLabel {
                vertex: 0
            }))
        );

        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let first_vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex");

        tri.set_vertex_data(&first_vertex, Some(7))
            .expect("Expected valid vertex handle while mutating label");
        assert_matches!(
            tri.synchronize_foliation_from_live_labels(),
            Err(CdtError::Foliation(FoliationError::OutOfRangeVertexLabel {
                vertex: 0,
                label: 7,
                expected_range_end: 2,
            }))
        );
    }

    #[test]
    fn validate_foliation_rejects_stored_label_count_mismatch() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        tri.foliation = Some(
            Foliation::from_slice_sizes(
                vec![1, 1],
                NonZeroU32::new(2).expect("test slice count should be nonzero"),
            )
            .expect("non-empty mismatched bookkeeping is constructible"),
        );

        assert_matches!(
            tri.validate_foliation(),
            Err(CdtError::Foliation(FoliationError::LabelCountMismatch {
                labeled: 2,
                expected: 3,
            }))
        );
    }

    #[test]
    fn assign_foliation_by_y_updates_metadata_invalidates_cache_and_writes_labels() {
        let mut tri = CdtTriangulation::from_seeded_points(15, 3, 2, 42)
            .expect("Failed to create deterministic triangulation");
        let initial_modification_count = tri.metadata().modification_count;
        let initial_edge_count = tri.edge_count();
        tri.refresh_cache();
        assert!(tri.cache.edge_count.is_some());

        let old_last_modified = tri
            .metadata()
            .last_modified
            .checked_sub(Duration::from_secs(1))
            .expect("test timestamp should permit a one-second offset");
        tri.metadata.last_modified = old_last_modified;
        let before_assignment = std::time::Instant::now();
        tri.assign_foliation_by_y(slice_count(3))
            .expect("Should assign foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.time_slices().get(), 3);
        assert_eq!(tri.slice_sizes().iter().sum::<usize>(), tri.vertex_count());
        assert!(tri.metadata().last_modified >= before_assignment);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count + 1
        );
        assert!(tri.cache.edge_count.is_none());
        assert_eq!(tri.edge_count(), initial_edge_count);
        for vh in tri.geometry().vertices() {
            assert!(tri.time_label(&vh).is_ok_and(|label| label.is_some()));
        }
    }

    #[test]
    fn assign_foliation_by_y_error_paths_preserve_state() {
        let mut tri = CdtTriangulation::from_seeded_points(6, 2, 2, TEST_POINT_SEED)
            .expect("Failed to create triangulation");
        let initial_time_slices = tri.time_slices();
        let initial_modification_count = tri.metadata().modification_count;
        let vertex_keys: Vec<_> = tri
            .geometry()
            .vertices()
            .map(|vh| vh.vertex_key())
            .collect();

        assert!(NonZeroU32::new(0).is_none());
        assert_eq!(tri.time_slices(), initial_time_slices);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count
        );

        let requested_slices = u32::try_from(tri.vertex_count())
            .expect("vertex count should fit into u32 for this test")
            .saturating_add(1);
        let result = tri.assign_foliation_by_y(slice_count(requested_slices));

        assert_matches!(
            result,
            Err(CdtError::Foliation(FoliationError::EmptySlice { .. }))
        );
        assert_eq!(tri.time_slices(), initial_time_slices);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count
        );
        assert!(!tri.has_foliation());
        for key in vertex_keys {
            assert_eq!(tri.geometry().vertex_data_by_key(key), None);
        }
    }

    #[test]
    fn assign_foliation_by_y_rejects_invalid_toroidal_slice_count() {
        let mut tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");
        let initial_slice_sizes = tri.slice_sizes().to_vec();

        let result = tri.assign_foliation_by_y(slice_count(2));

        assert_matches!(
            result,
            Err(CdtError::InvalidTriangulationMetadata {
                ref field,
                topology,
                ref provided_value,
                ref expected,
            }) if *field == TriangulationMetadataField::Timeslices
                && topology == CdtTopology::Toroidal
                && provided_value == "2"
                && expected == "≥ 3"
        );
        assert_eq!(tri.time_slices().get(), 3);
        assert_eq!(tri.slice_sizes(), initial_slice_sizes.as_slice());
        assert!(tri.has_foliation());
    }

    #[test]
    fn foliation_queries_report_current_labels_only() {
        let mut tri = CdtTriangulation::from_seeded_points(6, 1, 2, TEST_POINT_SEED)
            .expect("Failed to create triangulation");
        assert!(!tri.has_foliation());
        assert!(tri.foliation().is_none());
        assert!(tri.slice_sizes().is_empty());
        assert!(tri.vertices_at_time(0).next().is_none());

        tri.assign_foliation_by_y(slice_count(1))
            .expect("Should assign single-slice foliation");

        assert!(tri.has_foliation());
        assert_eq!(tri.slice_sizes(), &[tri.vertex_count()]);
        assert_eq!(tri.vertices_at_time(0).count(), tri.vertex_count());
        assert!(tri.vertices_at_time(999).next().is_none());
        for vh in tri.geometry().vertices() {
            assert_eq!(tri.time_label(&vh), Ok(Some(0)));
        }
    }

    #[test]
    fn stale_foliation_hides_public_queries_and_fails_validation() {
        let mut tri = strict_strip(4, 2);
        let vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("strip should contain vertices");
        let label = tri
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
            .expect("strip vertices should be labeled");
        let edge = tri
            .geometry()
            .edges()
            .next()
            .expect("strip should contain edges");
        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("strip should contain faces");

        tri.set_vertex_data(&vertex, Some(label))
            .expect("label rewrite should stale foliation bookkeeping");

        assert!(!tri.has_foliation());
        assert!(tri.foliation().is_none());
        assert_eq!(tri.time_label(&vertex), Ok(None));
        assert!(tri.vertices_at_time(label).next().is_none());
        assert_eq!(tri.edge_type(&edge), Ok(None));
        assert_eq!(tri.simplex_type(&face), Ok(None));
        assert_eq!(tri.face_edge_types(&face), Ok(None));
        assert_eq!(tri.simplex_type_from_data(&face), Ok(None));
        assert_matches!(
            tri.strict_causal_simplex_violation_count(),
            Err(CdtError::Foliation(FoliationError::StaleBookkeeping { .. }))
        );

        for result in [
            tri.validate_foliation(),
            tri.validate_causality(),
            tri.validate_simplex_classification(),
        ] {
            assert_matches!(
                result,
                Err(CdtError::Foliation(FoliationError::StaleBookkeeping { .. }))
            );
        }
    }

    #[test]
    fn face_and_simplex_classification_cover_foliated_and_unfoliated_states() {
        let tri = CdtTriangulation::from_seeded_points(5, 2, 2, TEST_POINT_SEED)
            .expect("create triangulation without foliation");
        for face in tri.geometry().faces() {
            assert_eq!(tri.face_edge_types(&face), Ok(None));
            assert_eq!(tri.simplex_type(&face), Ok(None));
        }
        tri.validate_simplex_classification()
            .expect("missing foliation should validate vacuously");
        assert_matches!(
            tri.strict_causal_simplex_violation_count(),
            Err(CdtError::Foliation(FoliationError::MissingBookkeeping))
        );

        let mut tri = strict_strip(5, 3);
        for face in tri.geometry().faces() {
            let edge_types = tri
                .face_edge_types(&face)
                .expect("Delaunay strip face query should succeed")
                .expect("Delaunay strip face should expose edge types");
            assert_eq!(
                edge_types
                    .iter()
                    .filter(|edge_type| matches!(edge_type, EdgeType::Spacelike))
                    .count(),
                1
            );
            assert_eq!(
                edge_types
                    .iter()
                    .filter(|edge_type| matches!(edge_type, EdgeType::Timelike))
                    .count(),
                2
            );
        }

        let classified = tri
            .classify_all_simplices()
            .expect("strict strip simplices should classify")
            .expect("foliation is present");
        assert_eq!(classified, tri.face_count());
    }

    #[test]
    fn classification_payloads_are_cleared_when_foliation_becomes_stale() {
        let backend = labeled_triangle_backend([0, 0, 1]);
        let mut tri = CdtTriangulation::from_labeled_delaunay(backend, 2, 2)
            .expect("Should preserve labels as foliation");
        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("Triangle should contain a face");
        assert_eq!(tri.simplex_type_from_data(&face), tri.simplex_type(&face));
        let live_ct = tri
            .simplex_type(&face)
            .expect("single face query should succeed")
            .expect("Single face should be classifiable");
        assert_matches!(live_ct, SimplexType::Up | SimplexType::Down);

        tri.classify_all_simplices()
            .expect("Should classify simplices with foliation")
            .expect("Foliation is present");
        assert_eq!(tri.simplex_type_from_data(&face), Ok(Some(live_ct)));

        let vertex_to_mutate = tri
            .geometry()
            .vertices()
            .next()
            .expect("Triangle should contain a vertex");
        tri.set_vertex_data(&vertex_to_mutate, Some(7))
            .expect("Expected valid vertex handle while mutating label");

        assert_eq!(tri.simplex_type_from_data(&face), Ok(None));
    }

    #[test]
    fn classification_rejects_same_slice_triangle() {
        let backend = labeled_triangle_backend([0, 0, 0]);

        assert_matches!(
            CdtTriangulation::from_labeled_delaunay(backend, 1, 2),
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
    fn strict_causal_simplex_violation_count_reports_non_strict_faces() {
        let backend = labeled_triangle_backend([0, 0, 0]);
        let mut tri = CdtTriangulation::try_new(backend, 1, 2)
            .expect("single Delaunay triangle should satisfy bare topology");
        tri.foliation = Some(
            Foliation::from_slice_sizes(vec![3], slice_count(1))
                .expect("single nonempty slice should be constructible"),
        );
        tri.mark_foliation_synchronized();

        assert_eq!(
            tri.strict_causal_simplex_violation_count()
                .expect("current foliation should count non-strict faces"),
            1
        );
        assert_matches!(
            tri.validate_simplex_classification(),
            Err(CdtError::ValidationFailed {
                check: CdtValidationCheck::SimplexClassification,
                failure: CdtValidationFailure::NonStrictSimplex { .. },
            })
        );
    }

    #[test]
    fn reassigning_foliation_clears_stale_simplex_payloads() {
        let mut tri =
            CdtTriangulation::from_cdt_strip(5, 3).expect("Failed to create deterministic strip");

        tri.assign_foliation_by_y(slice_count(3))
            .expect("First foliation assignment should succeed");
        tri.classify_all_simplices()
            .expect("classify_all_simplices should succeed")
            .expect("foliation is present");

        tri.assign_foliation_by_y(slice_count(2))
            .expect("Re-assignment with different slice count should succeed");

        assert_eq!(tri.time_slices().get(), 2);
        assert_eq!(tri.slice_sizes().len(), 2);
        assert_eq!(tri.slice_sizes().iter().sum::<usize>(), tri.vertex_count());
        for face in tri.geometry().faces() {
            assert_eq!(tri.simplex_type_from_data(&face), Ok(None));
        }
        assert_matches!(
            tri.validate_foliation(),
            Err(CdtError::Foliation(
                FoliationError::SpacelikeNonOpenInterval { .. }
                    | FoliationError::OpenBoundarySpatialOrderMismatch { .. }
                    | FoliationError::SpacelikeOpenSliceDegreeViolation { .. }
            ))
        );
    }

    #[test]
    fn volume_profile_counts_temporal_wrap_slab() {
        let tri = CdtTriangulation::from_toroidal_cdt(4, 3).expect("build toroidal CDT");

        let profile = tri
            .volume_profile()
            .expect("toroidal CDT should have a valid volume profile");

        assert_eq!(profile, vec![8, 8, 8]);
        assert_eq!(profile.iter().sum::<u32>(), 24);
    }

    #[test]
    fn volume_profile_is_empty_without_current_foliation() {
        let triangulation = CdtTriangulation::from_seeded_points(5, 3, 2, TEST_POINT_SEED)
            .expect("create unfoliated triangulation");

        assert!(
            triangulation
                .volume_profile()
                .expect("missing foliation should not fail")
                .is_empty()
        );

        let mut triangulation = strict_strip(4, 2);
        let vertex = triangulation
            .geometry()
            .vertices()
            .next()
            .expect("strip should contain vertices");
        let label = triangulation
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
            .expect("strip vertices should be labeled");
        triangulation
            .set_vertex_data(&vertex, Some(label))
            .expect("label rewrite should stale foliation bookkeeping");

        assert!(
            triangulation
                .volume_profile()
                .expect("stale foliation should not fail")
                .is_empty()
        );
    }
}
