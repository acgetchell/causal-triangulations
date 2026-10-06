#![forbid(unsafe_code)]

//! High-level triangulation operations.
//!
//! This module provides common operations that work across different
//! geometry backends.

use super::traits::TriangulationQuery;
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

/// Produces comparable endpoint hashes so unordered keys can avoid requiring [`Ord`].
fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// An unordered (undirected) pair key.
///
/// Used to treat edges as undirected without requiring an [`Ord`] bound on the handle type.
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
    // Map: facet key -> occurrence count. The key retains the representative vertices.
    type FacetCounts<V> = HashMap<UnorderedSet<V>, usize>;
    let mut facet_counts: FacetCounts<B::VertexHandle> = HashMap::new();

    for face in tri.faces() {
        let Ok(vertices) = tri.face_vertices(&face) else {
            continue;
        };
        let vertices: Vec<_> = vertices.collect();

        if vertices.len() < 2 {
            continue;
        }

        // Omitting each vertex also yields singleton boundary facets in 1D.
        for omit in 0..vertices.len() {
            let mut facet = Vec::with_capacity(vertices.len() - 1);
            facet.extend_from_slice(&vertices[..omit]);
            facet.extend_from_slice(&vertices[omit + 1..]);

            facet_counts
                .entry(UnorderedSet(facet))
                .and_modify(|count| *count += 1)
                .or_insert(1);
        }
    }

    facet_counts
        .into_iter()
        .filter_map(|(facet, count)| (count == 1).then_some(facet.0))
        .collect()
}

/// Common utility operations for triangulations.
///
/// Handle capabilities are constrained on individual operations so every
/// [`TriangulationQuery`] implementation receives the extension trait without
/// inheriting cloning, equality, or hashing requirements it does not use.
///
/// Structural validity does not establish the Delaunay property. Use
/// [`TriangulationQuery::is_valid`] for structure and
/// [`DelaunayBackend::is_delaunay`](super::backends::delaunay::DelaunayBackend::is_delaunay)
/// for the backend's Level 5 check. This extension trait does not supply a
/// Delaunay predicate for arbitrary query implementations.
///
/// ```compile_fail,E0599
/// use causal_triangulations::prelude::geometry::TriangulationOps;
///
/// fn check_delaunay<B: TriangulationOps>(backend: &B) -> bool {
///     backend.is_delaunay()
/// }
/// ```
pub trait TriangulationOps: TriangulationQuery {
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
                Ok((v1, v2)) => {
                    edge_by_vertices.insert(UnorderedPair(v1, v2), edge);
                }
                Err(error) => {
                    log::trace!("boundary_edges: skipping unresolved edge: {error}");
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
    use crate::geometry::traits::{EdgeAdjacentFacesResult, GeometryBackend};

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

        fn vertex_coordinates<'a>(
            &'a self,
            vertex: &Self::VertexHandle,
        ) -> Result<&'a [Self::Coordinate], Self::Error> {
            self.vertices
                .contains(vertex)
                .then_some(&[0.0][..])
                .ok_or(FixtureError::Vertex)
        }

        fn face_vertices<'a>(
            &'a self,
            face: &Self::FaceHandle,
        ) -> Result<impl ExactSizeIterator<Item = Self::VertexHandle> + 'a, Self::Error> {
            self.faces
                .iter()
                .find(|(candidate, _)| candidate == face)
                .and_then(|(_, vertices)| vertices.as_deref())
                .map(|vertices| vertices.iter().copied())
                .ok_or(FixtureError::Face)
        }

        fn edge_endpoints(
            &self,
            edge: &Self::EdgeHandle,
        ) -> Result<(Self::VertexHandle, Self::VertexHandle), Self::Error> {
            self.edges
                .iter()
                .find(|(candidate, _)| candidate == edge)
                .and_then(|(_, endpoints)| *endpoints)
                .ok_or(FixtureError::Edge)
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

        fn adjacent_faces<'a>(
            &'a self,
            vertex: &Self::VertexHandle,
        ) -> Result<impl Iterator<Item = Self::FaceHandle> + 'a, Self::Error> {
            self.vertices
                .contains(vertex)
                .then_some(std::iter::empty())
                .ok_or(FixtureError::Vertex)
        }

        fn incident_edges<'a>(
            &'a self,
            vertex: &Self::VertexHandle,
        ) -> Result<impl Iterator<Item = Self::EdgeHandle> + 'a, Self::Error> {
            self.vertices
                .contains(vertex)
                .then_some(std::iter::empty())
                .ok_or(FixtureError::Vertex)
        }

        fn face_neighbors<'a>(
            &'a self,
            face: &Self::FaceHandle,
        ) -> Result<impl Iterator<Item = Self::FaceHandle> + 'a, Self::Error> {
            self.faces
                .iter()
                .any(|(candidate, _)| candidate == face)
                .then_some(std::iter::empty())
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
            faces: vec![
                (0, Some(vec![0, 1])),
                (1, None),
                (2, Some(vec![2])),
                (3, Some(vec![1, 2])),
            ],
        };

        let hull: HashSet<_> = backend.convex_hull().into_iter().collect();
        assert_eq!(hull, HashSet::from([0, 2]));
        assert_eq!(backend.boundary_edges(), [] as [usize; 0]);
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
        assert_eq!(backend.convex_hull().len(), 3);
        assert_eq!(backend.boundary_edges().len(), 3);

        // Verify return types are as expected
        let hull: Vec<_> = backend.convex_hull();
        let boundary: Vec<_> = backend.boundary_edges();
        assert_eq!(hull.len(), 3);
        assert_eq!(boundary.len(), 3);
    }
}
