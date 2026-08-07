#![forbid(unsafe_code)]

//! High-level triangulation operations.
//!
//! This module provides common operations that work across different
//! geometry backends.

use super::traits::TriangulationQuery;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Produces comparable endpoint hashes so unordered keys can avoid requiring `Ord`.
fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// An unordered (undirected) pair key.
///
/// Used to treat edges as undirected without requiring an `Ord` bound on the handle type.
#[derive(Clone, Debug)]
struct UnorderedPair<V>(V, V);

impl<V: Eq> PartialEq for UnorderedPair<V> {
    fn eq(&self, other: &Self) -> bool {
        (self.0 == other.0 && self.1 == other.1) || (self.0 == other.1 && self.1 == other.0)
    }
}

impl<V: Eq> Eq for UnorderedPair<V> {}

impl<V: Hash> Hash for UnorderedPair<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Ensure order-independence by hashing both endpoints and writing the u64s sorted.
        let a = stable_hash(&self.0);
        let b = stable_hash(&self.1);

        if a <= b {
            state.write_u64(a);
            state.write_u64(b);
        } else {
            state.write_u64(b);
            state.write_u64(a);
        }
    }
}

/// An unordered set key (order-independent equality + hashing).
///
/// Used to match the same facet extracted from two adjacent simplices, even if vertex order differs.
#[derive(Clone, Debug)]
struct UnorderedSet<V>(Vec<V>);

impl<V: Eq> PartialEq for UnorderedSet<V> {
    fn eq(&self, other: &Self) -> bool {
        // Compare as sets (order-independent and duplicate-robust).
        self.0
            .iter()
            .all(|value| other.0.iter().any(|candidate| candidate == value))
            && other
                .0
                .iter()
                .all(|value| self.0.iter().any(|candidate| candidate == value))
    }
}

impl<V: Eq> Eq for UnorderedSet<V> {}

impl<V: Hash> Hash for UnorderedSet<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Order-independent, duplicate-robust hash by hashing each unique element and sorting.
        let mut hashes: Vec<u64> = self.0.iter().map(stable_hash).collect();
        hashes.sort_unstable();
        hashes.dedup();
        for h in hashes {
            state.write_u64(h);
        }
    }
}

/// Compute boundary facets (a.k.a. hull facets) of the simplicial complex.
///
/// For Delaunay simplices, a facet is the set of all simplex vertices excluding one vertex.
/// Any facet that appears in exactly one simplex is on the boundary.
fn boundary_facets<B>(tri: &B) -> Vec<Vec<B::VertexHandle>>
where
    B: TriangulationQuery + ?Sized,
    B::VertexHandle: Clone + Eq + Hash,
{
    // Map: facet key -> (occurrence count, representative vertex list)
    type FacetCounts<V> = HashMap<UnorderedSet<V>, (usize, Vec<V>)>;
    let mut facet_counts: FacetCounts<B::VertexHandle> = HashMap::new();

    for face in tri.faces() {
        let Ok(vertices) = tri.face_vertices(&face) else {
            continue;
        };

        if vertices.len() < 2 {
            continue;
        }

        // Degenerate 1D simplex (edge): treat each endpoint as a "facet" (0D boundary).
        if vertices.len() == 2 {
            for v in &vertices {
                let facet = vec![v.clone()];
                let key = UnorderedSet(facet.clone());
                facet_counts
                    .entry(key)
                    .and_modify(|(count, _)| *count += 1)
                    .or_insert((1, facet));
            }
            continue;
        }

        // Simplex facets: omit each vertex once.
        for omit in 0..vertices.len() {
            let facet: Vec<_> = vertices
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != omit)
                .map(|(_, v)| v.clone())
                .collect();

            let key = UnorderedSet(facet.clone());
            facet_counts
                .entry(key)
                .and_modify(|(count, _)| *count += 1)
                .or_insert((1, facet));
        }
    }

    facet_counts
        .into_values()
        .filter_map(|(count, facet)| (count == 1).then_some(facet))
        .collect()
}

/// Common utility operations for triangulations.
///
/// Handle capabilities are constrained on individual operations so every
/// [`TriangulationQuery`] implementation receives the extension trait without
/// inheriting cloning, equality, or hashing requirements it does not use.
pub trait TriangulationOps: TriangulationQuery {
    /// Check if the triangulation satisfies Delaunay property (if applicable)
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// assert!(backend.is_delaunay());
    /// ```
    fn is_delaunay(&self) -> bool {
        // Delegate to the backend's validation method
        // For Delaunay backends with appropriate trait bounds, this checks the
        // circumcircle property. For other backends, it checks basic validity.
        self.is_valid()
    }

    /// Compute the convex hull of the triangulation.
    ///
    /// Returns the set of vertices that lie on the boundary (convex hull) of the triangulation.
    ///
    /// # Notes
    /// - For 2D triangulations, these are the vertices incident to at least one boundary edge.
    /// - For higher dimensions, these are the vertices incident to at least one boundary facet.
    /// - The returned vertex order is **unspecified**.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// let hull = backend.convex_hull();
    /// assert_eq!(hull.len(), 3);
    /// ```
    fn convex_hull(&self) -> Vec<Self::VertexHandle>
    where
        Self::VertexHandle: Clone + Eq + Hash,
    {
        let mut hull_vertices: HashSet<Self::VertexHandle> = HashSet::new();

        for facet in boundary_facets(self) {
            for v in facet {
                hull_vertices.insert(v);
            }
        }

        hull_vertices.into_iter().collect()
    }

    /// Find all boundary edges of the triangulation.
    ///
    /// In 2D, these are the edges that are incident to exactly one face (triangle).
    /// In higher dimensions, these are the edges that appear in at least one boundary facet.
    ///
    /// # Notes
    /// - The returned edge order is **unspecified**.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::testing::*;
    ///
    /// let backend = MockBackend::create_triangle();
    /// let boundary = backend.boundary_edges();
    /// assert_eq!(boundary.len(), 3);
    /// ```
    fn boundary_edges(&self) -> Vec<Self::EdgeHandle>
    where
        Self::VertexHandle: Clone + Eq + Hash,
        Self::EdgeHandle: Clone + Eq + Hash,
    {
        // Build a lookup from an (unordered) vertex pair to the corresponding edge handle.
        let mut edge_by_vertices: HashMap<UnorderedPair<Self::VertexHandle>, Self::EdgeHandle> =
            HashMap::new();

        for edge in self.edges() {
            match self.edge_endpoints(&edge) {
                Some((v1, v2)) => {
                    edge_by_vertices.insert(UnorderedPair(v1, v2), edge);
                }
                None => {
                    log::trace!("boundary_edges: skipping unresolved edge");
                }
            }
        }

        // Collect all edges that lie on any boundary facet.
        let mut boundary: HashSet<Self::EdgeHandle> = HashSet::new();

        for facet in boundary_facets(self) {
            // For a facet with k vertices, include all k-choose-2 edges on that facet.
            for i in 0..facet.len() {
                for j in (i + 1)..facet.len() {
                    let key = UnorderedPair(facet[i].clone(), facet[j].clone());
                    if let Some(edge) = edge_by_vertices.get(&key) {
                        boundary.insert(edge.clone());
                    }
                }
            }
        }

        boundary.into_iter().collect()
    }
}

// Blanket implementation for all types that implement TriangulationQuery
impl<T: TriangulationQuery + ?Sized> TriangulationOps for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::backends::mock::MockBackend;
    use crate::geometry::traits::{EdgeAdjacentFacesResult, GeometryBackend, TriangulationQuery};
    use std::assert_matches;
    use std::collections::HashSet;

    #[derive(Debug, Clone)]
    struct FixtureBackend {
        vertices: Vec<usize>,
        edges: Vec<(usize, Option<(usize, usize)>)>,
        faces: Vec<(usize, Option<Vec<usize>>)>,
    }

    #[derive(Debug, PartialEq, Eq, thiserror::Error)]
    enum FixtureError {
        #[error("invalid face")]
        Face,
        #[error("invalid vertex")]
        Vertex,
        #[error("invalid edge")]
        Edge,
    }

    impl GeometryBackend for FixtureBackend {
        type Coordinate = f64;
        type VertexHandle = usize;
        type EdgeHandle = usize;
        type FaceHandle = usize;
        type Error = FixtureError;

        fn backend_name(&self) -> &'static str {
            "fixture"
        }
    }

    impl TriangulationQuery for FixtureBackend {
        fn vertex_count(&self) -> usize {
            self.vertices.len()
        }

        fn edge_count(&self) -> usize {
            self.edges.len()
        }

        fn face_count(&self) -> usize {
            self.faces.len()
        }

        fn dimension(&self) -> usize {
            1
        }

        fn vertices(&self) -> impl Iterator<Item = Self::VertexHandle> + '_ {
            self.vertices.iter().copied()
        }

        fn edges(&self) -> impl Iterator<Item = Self::EdgeHandle> + '_ {
            self.edges.iter().map(|(edge, _)| *edge)
        }

        fn faces(&self) -> impl Iterator<Item = Self::FaceHandle> + '_ {
            self.faces.iter().map(|(face, _)| *face)
        }

        fn vertex_coordinates(
            &self,
            vertex: &Self::VertexHandle,
        ) -> Result<Vec<Self::Coordinate>, Self::Error> {
            self.vertices
                .contains(vertex)
                .then_some(vec![0.0])
                .ok_or(FixtureError::Vertex)
        }

        fn face_vertices(
            &self,
            face: &Self::FaceHandle,
        ) -> Result<Vec<Self::VertexHandle>, Self::Error> {
            self.faces
                .iter()
                .find(|(candidate, _)| candidate == face)
                .and_then(|(_, vertices)| vertices.clone())
                .ok_or(FixtureError::Face)
        }

        fn edge_endpoints(
            &self,
            edge: &Self::EdgeHandle,
        ) -> Option<(Self::VertexHandle, Self::VertexHandle)> {
            self.edges
                .iter()
                .find(|(candidate, _)| candidate == edge)
                .and_then(|(_, endpoints)| *endpoints)
        }

        fn edge_adjacent_faces(
            &self,
            edge: &Self::EdgeHandle,
        ) -> EdgeAdjacentFacesResult<Self::VertexHandle, Self::FaceHandle, Self::Error> {
            self.edges
                .iter()
                .any(|(candidate, _)| candidate == edge)
                .then_some(None)
                .ok_or(FixtureError::Edge)
        }

        fn adjacent_faces(
            &self,
            vertex: &Self::VertexHandle,
        ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
            self.vertices
                .contains(vertex)
                .then_some(Vec::new())
                .ok_or(FixtureError::Vertex)
        }

        fn incident_edges(
            &self,
            vertex: &Self::VertexHandle,
        ) -> Result<Vec<Self::EdgeHandle>, Self::Error> {
            self.vertices
                .contains(vertex)
                .then_some(Vec::new())
                .ok_or(FixtureError::Vertex)
        }

        fn face_neighbors(
            &self,
            face: &Self::FaceHandle,
        ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
            self.faces
                .iter()
                .any(|(candidate, _)| candidate == face)
                .then_some(Vec::new())
                .ok_or(FixtureError::Face)
        }

        fn is_valid(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_unordered_set_is_order_independent_and_duplicate_robust() {
        let key = UnorderedSet(vec![1_u8, 2, 3]);
        let reversed = UnorderedSet(vec![3_u8, 2, 1]);
        let shorter = UnorderedSet(vec![1_u8, 2]);
        let deduped = UnorderedSet(vec![1_u8, 2]);
        let duplicated = UnorderedSet(vec![1_u8, 1, 2]);

        assert_eq!(key, reversed);
        assert_ne!(key, shorter);
        assert_eq!(deduped, duplicated);

        let mut set = HashSet::new();
        set.insert(key);
        assert!(set.contains(&reversed));
        assert!(!set.contains(&shorter));
        set.insert(deduped);
        assert!(set.contains(&duplicated));
    }

    #[test]
    fn test_boundary_facets_skip_invalid_and_degenerate_faces() {
        let backend = FixtureBackend {
            vertices: vec![0, 1, 2],
            edges: vec![(0, Some((0, 1))), (1, None)],
            faces: vec![(0, Some(vec![0, 1])), (1, None), (2, Some(vec![2]))],
        };

        assert_eq!(backend.backend_name(), "fixture");
        assert_eq!(backend.vertex_count(), 3);
        assert_eq!(backend.edge_count(), 2);
        assert_eq!(backend.face_count(), 3);
        assert_eq!(backend.dimension(), 1);
        assert_eq!(backend.vertices().collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(backend.vertex_coordinates(&0), Ok(vec![0.0]));
        assert_matches!(backend.vertex_coordinates(&99), Err(FixtureError::Vertex));
        assert_eq!(backend.adjacent_faces(&0), Ok(Vec::new()));
        assert_matches!(backend.adjacent_faces(&99), Err(FixtureError::Vertex));
        assert_eq!(backend.incident_edges(&0), Ok(Vec::new()));
        assert_matches!(backend.incident_edges(&99), Err(FixtureError::Vertex));
        assert_eq!(backend.face_neighbors(&0), Ok(Vec::new()));
        assert_matches!(backend.face_neighbors(&99), Err(FixtureError::Face));

        let hull: HashSet<_> = backend.convex_hull().into_iter().collect();
        assert_eq!(hull, HashSet::from([0, 1]));
        assert!(backend.boundary_edges().is_empty());
    }

    #[test]
    fn test_is_delaunay_delegates_to_is_valid() {
        let backend = FixtureBackend {
            vertices: vec![0],
            edges: vec![],
            faces: vec![],
        };

        // The default is_delaunay implementation delegates to is_valid.
        assert!(backend.is_delaunay());
    }

    #[test]
    fn test_convex_hull_triangle() {
        let backend = MockBackend::create_triangle();

        let hull = backend.convex_hull();
        assert_eq!(hull.len(), 3, "Triangle hull should contain 3 vertices");

        let all_vertices: HashSet<_> = backend.vertices().collect();
        let hull_vertices: HashSet<_> = hull.into_iter().collect();
        assert_eq!(
            hull_vertices, all_vertices,
            "Hull vertices should match the triangulation's vertex set for a single triangle"
        );
    }

    #[test]
    fn test_boundary_edges_triangle() {
        let backend = MockBackend::create_triangle();

        let boundary = backend.boundary_edges();
        assert_eq!(boundary.len(), 3, "Triangle should have 3 boundary edges");

        let vertices: HashSet<_> = backend.vertices().collect();
        for edge in boundary {
            let (v1, v2) = backend
                .edge_endpoints(&edge)
                .expect("Boundary edge handle should be valid");
            assert!(
                vertices.contains(&v1) && vertices.contains(&v2),
                "Boundary edge endpoints should be valid vertices"
            );
            assert_ne!(v1, v2, "Boundary edge should not be degenerate");
        }
    }

    #[test]
    fn test_triangulation_ops_trait_available() {
        let backend = MockBackend::create_triangle();

        // Verify the blanket implementation provides all trait methods with expected types
        assert!(backend.is_delaunay()); // Should delegate to is_valid() for mock backend
        assert_eq!(backend.convex_hull().len(), 3);
        assert_eq!(backend.boundary_edges().len(), 3);

        // Verify return types are as expected
        let hull: Vec<_> = backend.convex_hull();
        let boundary: Vec<_> = backend.boundary_edges();
        assert_eq!(hull.len(), 3);
        assert_eq!(boundary.len(), 3);
    }
}
