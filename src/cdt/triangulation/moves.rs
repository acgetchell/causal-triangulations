#![forbid(unsafe_code)]

//! Narrow CDT-owned move-support hooks over the Delaunay backend.

use super::CdtTriangulation;
use crate::geometry::DelaunayBackend2D;
use crate::geometry::backends::delaunay::{
    DelaunayEdgeHandle, DelaunayError, DelaunayFaceHandle, DelaunayVertexHandle,
};
use crate::geometry::traits::{FlipResult, SubdivisionResult, TriangulationMut};

impl CdtTriangulation<DelaunayBackend2D> {
    /// Flips an edge and marks CDT-derived state stale when the backend mutation succeeds.
    pub(crate) fn flip_edge(
        &mut self,
        edge: DelaunayEdgeHandle,
    ) -> Result<FlipResult<DelaunayEdgeHandle, DelaunayFaceHandle>, DelaunayError> {
        let result = self.geometry.flip_edge(edge)?;
        self.bump_modification_count();
        Ok(result)
    }

    /// Subdivides a face and marks CDT-derived state stale when the backend mutation succeeds.
    pub(crate) fn subdivide_face(
        &mut self,
        face: DelaunayFaceHandle,
        point: &[f64],
    ) -> Result<SubdivisionResult<DelaunayVertexHandle, DelaunayFaceHandle>, DelaunayError> {
        let result = self.geometry.subdivide_face(face, point)?;
        self.bump_modification_count();
        Ok(result)
    }

    /// Removes a vertex and marks CDT-derived state stale when the backend mutation succeeds.
    pub(crate) fn remove_vertex(
        &mut self,
        vertex: DelaunayVertexHandle,
    ) -> Result<Vec<DelaunayFaceHandle>, DelaunayError> {
        let result = self.geometry.remove_vertex(vertex)?;
        self.bump_modification_count();
        Ok(result)
    }

    /// Updates a vertex time label and marks CDT-derived state stale on success.
    pub(crate) fn set_vertex_data(
        &mut self,
        vertex: &DelaunayVertexHandle,
        data: Option<u32>,
    ) -> Result<(), DelaunayError> {
        self.geometry
            .set_vertex_data_by_key(vertex.vertex_key(), data)?;
        self.bump_modification_count();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::traits::{TriangulationMut, TriangulationQuery};

    /// Computes the centroid of a live triangular face.
    fn face_centroid(triangulation: &CdtTriangulation<DelaunayBackend2D>) -> Vec<f64> {
        let face = triangulation
            .geometry()
            .faces()
            .next()
            .expect("triangulation should contain a face");
        let vertices = triangulation
            .geometry()
            .face_vertices(&face)
            .expect("face vertices should resolve");

        let mut centroid = vec![0.0; 2];
        for vertex in vertices {
            let coords = triangulation
                .geometry()
                .vertex_coordinates(&vertex)
                .expect("vertex coordinates should resolve");
            centroid[0] += coords[0] / 3.0;
            centroid[1] += coords[1] / 3.0;
        }
        centroid
    }

    #[test]
    fn set_vertex_data_marks_foliation_stale_and_invalidates_cache() {
        let mut tri = CdtTriangulation::from_cdt_strip(4, 2).expect("build explicit strip");
        let vertex = tri
            .geometry()
            .vertices()
            .next()
            .expect("strip should contain vertices");
        let label = tri
            .geometry()
            .vertex_data_by_key(vertex.vertex_key())
            .expect("strip vertices should be labeled");

        tri.refresh_cache();
        assert!(tri.cache.edge_count.is_some());
        let initial_modification_count = tri.metadata().modification_count;

        tri.set_vertex_data(&vertex, Some(label))
            .expect("label update should succeed");

        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count + 1
        );
        assert!(tri.cache.edge_count.is_none());
        assert!(!tri.has_foliation());
        assert!(tri.slice_sizes().is_empty());
    }

    #[test]
    fn subdivide_and_remove_vertex_each_invalidate_cached_counts() {
        let mut tri =
            CdtTriangulation::from_seeded_points(8, 1, 2, 53).expect("build triangulation");
        let face = tri
            .geometry()
            .faces()
            .next()
            .expect("triangulation should contain a face");
        let point = face_centroid(&tri);

        tri.refresh_cache();
        let initial_vertices = tri.vertex_count();
        let initial_modification_count = tri.metadata().modification_count;

        let subdivision = tri
            .subdivide_face(face, &point)
            .expect("centroid subdivision should succeed");

        assert_eq!(tri.vertex_count(), initial_vertices + 1);
        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count + 1
        );
        assert!(tri.cache.edge_count.is_none());

        tri.refresh_cache();
        assert!(tri.cache.edge_count.is_some());
        let after_subdivision_count = tri.metadata().modification_count;

        tri.remove_vertex(subdivision.new_vertex.clone())
            .expect("new subdivision vertex should be removable");

        assert_eq!(tri.vertex_count(), initial_vertices);
        assert_eq!(
            tri.metadata().modification_count,
            after_subdivision_count + 1
        );
        assert!(tri.cache.edge_count.is_none());

        let after_removal_count = tri.metadata().modification_count;
        assert!(
            tri.set_vertex_data(&subdivision.new_vertex, Some(0))
                .is_err()
        );
        assert_eq!(tri.metadata().modification_count, after_removal_count);
    }

    #[test]
    fn flip_edge_invalidates_cached_counts_when_backend_accepts_flip() {
        let mut tri =
            CdtTriangulation::from_seeded_points(8, 1, 2, 53).expect("build triangulation");
        let edge = tri
            .geometry()
            .edges()
            .find(|edge| tri.geometry().can_flip_edge(edge))
            .expect("seeded triangulation should contain a flippable edge");

        tri.refresh_cache();
        let initial_modification_count = tri.metadata().modification_count;

        tri.flip_edge(edge).expect("flippable edge should flip");

        assert_eq!(
            tri.metadata().modification_count,
            initial_modification_count + 1
        );
        assert!(tri.cache.edge_count.is_none());
    }
}
