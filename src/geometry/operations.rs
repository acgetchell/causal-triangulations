//! High-level triangulation operations.
//!
//! This module provides common operations that work across different
//! geometry backends.

use super::traits::TriangulationQuery;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

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
/// Used to match the same facet extracted from two adjacent cells, even if vertex order differs.
#[derive(Clone, Debug)]
struct UnorderedSet<V>(Vec<V>);

impl<V: Eq + Hash> PartialEq for UnorderedSet<V> {
    fn eq(&self, other: &Self) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }

        // Compare as sets (order-independent). Facet vertex lists should not contain duplicates.
        let self_set: HashSet<_> = self.0.iter().collect();
        other.0.iter().all(|v| self_set.contains(v))
    }
}

impl<V: Eq + Hash> Eq for UnorderedSet<V> {}

impl<V: Hash> Hash for UnorderedSet<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Order-independent hash by hashing each element and sorting the resulting u64s.
        let mut hashes: Vec<u64> = self.0.iter().map(stable_hash).collect();
        hashes.sort_unstable();
        for h in hashes {
            state.write_u64(h);
        }
    }
}

/// Compute boundary facets (a.k.a. hull facets) of the simplicial complex.
///
/// For simplices (Delaunay cells), a facet is the set of all cell vertices excluding one vertex.
/// Any facet that appears in exactly one cell is on the boundary.
fn boundary_facets<B: TriangulationQuery + ?Sized>(tri: &B) -> Vec<Vec<B::VertexHandle>> {
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

        // Degenerate 1D cell (edge): treat each endpoint as a "facet" (0D boundary).
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

/// Common utility operations for triangulations
pub trait TriangulationOps: TriangulationQuery {
    /// Check if the triangulation satisfies Delaunay property (if applicable)
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
    fn convex_hull(&self) -> Vec<Self::VertexHandle> {
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
    fn boundary_edges(&self) -> Vec<Self::EdgeHandle> {
        // Build a lookup from an (unordered) vertex pair to the corresponding edge handle.
        let mut edge_by_vertices: HashMap<UnorderedPair<Self::VertexHandle>, Self::EdgeHandle> =
            HashMap::new();

        for edge in self.edges() {
            if let Ok((v1, v2)) = self.edge_endpoints(&edge) {
                edge_by_vertices.insert(UnorderedPair(v1, v2), edge);
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
impl<T: TriangulationQuery> TriangulationOps for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::backends::mock::MockBackend;
    use crate::geometry::traits::TriangulationQuery;
    use std::collections::HashSet;

    #[test]
    fn test_is_delaunay_delegates_to_is_valid() {
        let backend = MockBackend::create_triangle();

        // For our mock backend, is_delaunay should delegate to is_valid
        // which returns true for a basic triangle
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
