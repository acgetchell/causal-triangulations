//! Mock geometry backend for testing.
//!
//! This backend provides a simple, controllable implementation for unit testing
//! CDT algorithms without requiring actual triangulation computations.

use crate::geometry::traits::{
    FlipResult, GeometryBackend, SubdivisionResult, TriangulationMut, TriangulationQuery,
};
use std::collections::HashMap;

/// Mock backend for testing
#[derive(Debug, Clone)]
pub struct MockBackend {
    vertices: HashMap<usize, Vec<f64>>,
    edges: HashMap<usize, (usize, usize)>,
    faces: HashMap<usize, Vec<usize>>,
    dimension: usize,
    next_vertex_id: usize,
    next_edge_id: usize,
    next_face_id: usize,
}

/// Mock vertex handle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MockVertexHandle(usize);

/// Mock edge handle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MockEdgeHandle(usize);

/// Mock face handle
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MockFaceHandle(usize);

/// Mock backend errors
#[derive(Debug, thiserror::Error)]
pub enum MockError {
    /// Invalid vertex handle provided
    #[error("Invalid vertex handle: {0}")]
    Vertex(usize),

    /// Invalid edge handle provided
    #[error("Invalid edge handle: {0}")]
    Edge(usize),

    /// Invalid face handle provided
    #[error("Invalid face handle: {0}")]
    Face(usize),

    /// Invalid operation attempted
    #[error("Invalid operation: {0}")]
    Operation(String),
}

impl MockBackend {
    /// Create a new mock backend
    #[must_use]
    pub fn new(dimension: usize) -> Self {
        Self {
            vertices: HashMap::new(),
            edges: HashMap::new(),
            faces: HashMap::new(),
            dimension,
            next_vertex_id: 0,
            next_edge_id: 0,
            next_face_id: 0,
        }
    }

    /// Create a simple triangle for testing
    #[must_use]
    pub fn create_triangle() -> Self {
        let mut backend = Self::new(2);

        // Add three vertices
        backend.vertices.insert(0, vec![0.0, 0.0]);
        backend.vertices.insert(1, vec![1.0, 0.0]);
        backend.vertices.insert(2, vec![0.5, 1.0]);
        backend.next_vertex_id = 3;

        // Add three edges
        backend.edges.insert(0, (0, 1));
        backend.edges.insert(1, (1, 2));
        backend.edges.insert(2, (2, 0));
        backend.next_edge_id = 3;

        // Add one face
        backend.faces.insert(0, vec![0, 1, 2]);
        backend.next_face_id = 1;

        backend
    }
}

impl GeometryBackend for MockBackend {
    type Coordinate = f64;
    type VertexHandle = MockVertexHandle;
    type EdgeHandle = MockEdgeHandle;
    type FaceHandle = MockFaceHandle;
    type Error = MockError;

    fn backend_name(&self) -> &'static str {
        "mock"
    }
}

impl TriangulationQuery for MockBackend {
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
        self.dimension
    }

    fn vertices(&self) -> Box<dyn Iterator<Item = Self::VertexHandle> + '_> {
        Box::new(self.vertices.keys().map(|&id| MockVertexHandle(id)))
    }

    fn edges(&self) -> Box<dyn Iterator<Item = Self::EdgeHandle> + '_> {
        Box::new(self.edges.keys().map(|&id| MockEdgeHandle(id)))
    }

    fn faces(&self) -> Box<dyn Iterator<Item = Self::FaceHandle> + '_> {
        Box::new(self.faces.keys().map(|&id| MockFaceHandle(id)))
    }

    fn vertex_coordinates(
        &self,
        vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::Coordinate>, Self::Error> {
        self.vertices
            .get(&vertex.0)
            .cloned()
            .ok_or(MockError::Vertex(vertex.0))
    }

    fn face_vertices(
        &self,
        face: &Self::FaceHandle,
    ) -> Result<Vec<Self::VertexHandle>, Self::Error> {
        self.faces
            .get(&face.0)
            .map(|indices| indices.iter().map(|&id| MockVertexHandle(id)).collect())
            .ok_or(MockError::Face(face.0))
    }

    fn edge_endpoints(
        &self,
        edge: &Self::EdgeHandle,
    ) -> Option<(Self::VertexHandle, Self::VertexHandle)> {
        self.edges
            .get(&edge.0)
            .map(|&(v1, v2)| (MockVertexHandle(v1), MockVertexHandle(v2)))
    }

    fn adjacent_faces(
        &self,
        _vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        // Simplified implementation
        Ok(Vec::new())
    }

    fn incident_edges(
        &self,
        _vertex: &Self::VertexHandle,
    ) -> Result<Vec<Self::EdgeHandle>, Self::Error> {
        // Simplified implementation
        Ok(Vec::new())
    }

    fn face_neighbors(
        &self,
        _face: &Self::FaceHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        // Simplified implementation
        Ok(Vec::new())
    }

    fn is_valid(&self) -> bool {
        !self.vertices.is_empty() && !self.faces.is_empty()
    }
}

impl TriangulationMut for MockBackend {
    fn insert_vertex(
        &mut self,
        coords: &[Self::Coordinate],
    ) -> Result<Self::VertexHandle, Self::Error> {
        let id = self.next_vertex_id;
        self.next_vertex_id += 1;
        self.vertices.insert(id, coords.to_vec());
        Ok(MockVertexHandle(id))
    }

    fn remove_vertex(
        &mut self,
        vertex: Self::VertexHandle,
    ) -> Result<Vec<Self::FaceHandle>, Self::Error> {
        self.vertices
            .remove(&vertex.0)
            .ok_or(MockError::Vertex(vertex.0))?;
        Ok(Vec::new())
    }

    fn move_vertex(
        &mut self,
        vertex: Self::VertexHandle,
        new_coords: &[Self::Coordinate],
    ) -> Result<(), Self::Error> {
        self.vertices
            .get_mut(&vertex.0)
            .map_or(Err(MockError::Vertex(vertex.0)), |coords| {
                *coords = new_coords.to_vec();
                Ok(())
            })
    }

    fn flip_edge(
        &mut self,
        edge: Self::EdgeHandle,
    ) -> Result<FlipResult<Self::VertexHandle, Self::EdgeHandle, Self::FaceHandle>, Self::Error>
    {
        if !self.edges.contains_key(&edge.0) {
            return Err(MockError::Edge(edge.0));
        }
        // Simplified implementation
        Ok(FlipResult::new(edge, Vec::new()))
    }

    fn can_flip_edge(&self, _edge: &Self::EdgeHandle) -> bool {
        true
    }

    fn subdivide_face(
        &mut self,
        face: Self::FaceHandle,
        point: &[Self::Coordinate],
    ) -> Result<
        SubdivisionResult<Self::VertexHandle, Self::EdgeHandle, Self::FaceHandle>,
        Self::Error,
    > {
        if !self.faces.contains_key(&face.0) {
            return Err(MockError::Face(face.0));
        }
        // Simplified implementation
        let new_vertex = self.insert_vertex(point)?;
        Ok(SubdivisionResult::new(new_vertex, Vec::new(), face))
    }

    fn clear(&mut self) {
        self.vertices.clear();
        self.edges.clear();
        self.faces.clear();
        self.next_vertex_id = 0;
        self.next_edge_id = 0;
        self.next_face_id = 0;
    }

    fn reserve_capacity(&mut self, _vertices: usize, _faces: usize) {
        // No-op for HashMap-based storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_backend_creation() {
        let backend = MockBackend::new(2);
        assert_eq!(backend.dimension(), 2);
        assert_eq!(backend.vertex_count(), 0);
    }

    #[test]
    fn test_mock_triangle() {
        let backend = MockBackend::create_triangle();
        assert_eq!(backend.vertex_count(), 3);
        assert_eq!(backend.edge_count(), 3);
        assert_eq!(backend.face_count(), 1);
        assert!(backend.is_valid());
    }
}
