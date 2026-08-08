#![forbid(unsafe_code)]

//! Compile contracts for minimal public generic bounds.

use causal_triangulations::prelude::geometry::{
    DelaunayBackend, EdgeAdjacentFacesResult, GeometryBackend, TriangulationOps, TriangulationQuery,
};
use causal_triangulations::prelude::simulation::{
    ActionConfig, CdtMoveFamilyPolicy, CdtProposal, CdtResult, CdtTriangulation,
    CdtTriangulation2D, MetropolisAlgorithm, MetropolisConfig, MoveType,
    UniformCdtMoveFamilyPolicy,
};
use serde::Serialize;
use std::iter::empty;

struct ExactCoordinate;
struct MinimalVertexHandle;
struct MinimalEdgeHandle;
struct MinimalFaceHandle;

#[derive(Debug, thiserror::Error)]
#[error("minimal backend query is unsupported")]
struct MinimalBackendError;

struct MinimalBackend;

impl GeometryBackend for MinimalBackend {
    type Coordinate = ExactCoordinate;
    type VertexHandle = MinimalVertexHandle;
    type EdgeHandle = MinimalEdgeHandle;
    type FaceHandle = MinimalFaceHandle;
    type Error = MinimalBackendError;

    fn backend_name(&self) -> &'static str {
        "minimal"
    }
}

impl TriangulationQuery for MinimalBackend {
    fn vertex_count(&self) -> usize {
        3
    }

    fn edge_count(&self) -> usize {
        3
    }

    fn face_count(&self) -> usize {
        1
    }

    fn dimension(&self) -> usize {
        2
    }

    fn vertices(&self) -> impl Iterator<Item = Self::VertexHandle> + '_ {
        empty()
    }

    fn edges(&self) -> impl Iterator<Item = Self::EdgeHandle> + '_ {
        empty()
    }

    fn faces(&self) -> impl Iterator<Item = Self::FaceHandle> + '_ {
        empty()
    }

    fn vertex_coordinates<'a>(
        &'a self,
        _vertex: &Self::VertexHandle,
    ) -> Result<&'a [Self::Coordinate], Self::Error> {
        Err(MinimalBackendError)
    }

    fn face_vertices<'a>(
        &'a self,
        _face: &Self::FaceHandle,
    ) -> Result<impl ExactSizeIterator<Item = Self::VertexHandle> + 'a, Self::Error> {
        Err::<std::iter::Empty<Self::VertexHandle>, Self::Error>(MinimalBackendError)
    }

    fn edge_endpoints(
        &self,
        _edge: &Self::EdgeHandle,
    ) -> Result<(Self::VertexHandle, Self::VertexHandle), Self::Error> {
        Err(MinimalBackendError)
    }

    fn edge_adjacent_faces(
        &self,
        _edge: &Self::EdgeHandle,
    ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error> {
        Ok(None)
    }

    fn adjacent_faces<'a>(
        &'a self,
        _vertex: &Self::VertexHandle,
    ) -> Result<impl Iterator<Item = Self::FaceHandle> + 'a, Self::Error> {
        Ok(empty())
    }

    fn incident_edges<'a>(
        &'a self,
        _vertex: &Self::VertexHandle,
    ) -> Result<impl Iterator<Item = Self::EdgeHandle> + 'a, Self::Error> {
        Ok(empty())
    }

    fn face_neighbors<'a>(
        &'a self,
        _face: &Self::FaceHandle,
    ) -> Result<impl Iterator<Item = Self::FaceHandle> + 'a, Self::Error> {
        Ok(empty())
    }

    fn is_valid(&self) -> bool {
        true
    }
}

const fn assert_query_has_operations<T: TriangulationQuery + ?Sized>() {
    const fn assert_operations<T: TriangulationOps + ?Sized>() {}
    assert_operations::<T>();
}

#[derive(Clone)]
struct CloneOnly;

#[derive(Serialize)]
struct SerializeOnly;

struct NoPayloadCapabilities;

fn inspect_unbounded_delaunay_backend<VertexData, SimplexData, const D: usize>(
    backend: &mut DelaunayBackend<VertexData, SimplexData, D>,
) {
    let _ = backend.triangulation();
    backend.set_delaunay_check_interval(None);
    let _ = backend.topology_kind();
    let _ = backend.periodic_domain();
}

fn inspect_policy_without_requiring_its_type<P>(
    proposal: &mut CdtProposal<P>,
    state: &CdtTriangulation2D,
) -> MoveType {
    proposal.policy_view(state, MoveType::Move13Add).family()
}

#[test]
fn basic_queries_do_not_require_numeric_or_handle_capabilities() -> CdtResult<()> {
    assert_query_has_operations::<MinimalBackend>();

    let triangulation = CdtTriangulation::try_new(MinimalBackend, 2, 2)?;
    assert_eq!(triangulation.vertex_count(), 3);
    assert_eq!(triangulation.edge_count(), 3);
    assert_eq!(triangulation.face_count(), 1);
    Ok(())
}

#[test]
fn delaunay_capabilities_use_their_narrowest_payload_bounds() {
    fn assert_clone<T: Clone>() {}
    fn assert_serialize<T: Serialize>() {}
    fn assert_geometry_backend<T: GeometryBackend>() {}

    assert_clone::<DelaunayBackend<CloneOnly, CloneOnly, 2>>();
    assert_serialize::<DelaunayBackend<SerializeOnly, SerializeOnly, 2>>();
    assert_geometry_backend::<DelaunayBackend<NoPayloadCapabilities, NoPayloadCapabilities, 2>>();
    let _ = inspect_unbounded_delaunay_backend::<NoPayloadCapabilities, NoPayloadCapabilities, 2>;
}

#[test]
fn proposal_inspection_does_not_add_a_policy_bound_to_generic_helpers() -> CdtResult<()> {
    let state = CdtTriangulation::from_cdt_strip(4, 3)?;
    let mut proposal = CdtProposal::new(ActionConfig::default());

    assert_eq!(
        inspect_policy_without_requiring_its_type(&mut proposal, &state),
        MoveType::Move13Add
    );
    Ok(())
}

#[test]
fn simulation_policy_arguments_accept_trait_objects() -> CdtResult<()> {
    let policy: &dyn CdtMoveFamilyPolicy = &UniformCdtMoveFamilyPolicy;
    let algorithm = MetropolisAlgorithm::new(
        MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
        ActionConfig::default(),
    );

    let results = algorithm
        .with_policy(policy)
        .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
    assert_eq!(results.steps().len(), 1);
    Ok(())
}
