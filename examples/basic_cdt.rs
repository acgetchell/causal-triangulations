#![forbid(unsafe_code)]

//! Basic example of using the Causal Dynamical Triangulations library
//!
//! This example shows how to:
//! - Create a CDT triangulation programmatically
//! - Configure simulation parameters
//! - Attempt a basic CDT simulation
//! - Run the Metropolis loop over real 2D CDT move kernels

use causal_triangulations::prelude::errors::CdtResult;
use causal_triangulations::prelude::simulation::{CdtConfig, MetropolisAlgorithm};
use causal_triangulations::prelude::triangulation::CdtTriangulation;
use log::{LevelFilter, info};

fn main() -> CdtResult<()> {
    // Initialize logging
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .init();

    info!("Starting basic CDT example");

    // Configuration parameters
    let vertices_per_slice = 16;
    let timeslices = 4;
    let vertices = vertices_per_slice * timeslices;

    // Create initial triangulation
    info!("Creating initial triangulation with {vertices} vertices and {timeslices} timeslices");
    let triangulation = CdtTriangulation::from_cdt_strip(vertices_per_slice, timeslices)?;

    info!(
        "Initial triangulation: {} vertices, {} edges, {} faces",
        triangulation.vertex_count(),
        triangulation.edge_count(),
        triangulation.face_count()
    );

    // Create a configuration for the simulation
    let mut config = CdtConfig::new(vertices, timeslices);
    config.temperature = 1.0;
    config.steps = 100;
    config.thermalization_steps = 20;
    config.measurement_frequency = 5;
    let config = config.into_validated()?;

    // Extract individual configs
    let metropolis_config = config.to_metropolis_config();
    let action_config = config.to_action_config();

    info!("Attempting CDT simulation...");
    let algorithm = MetropolisAlgorithm::new(metropolis_config, action_config);
    let results = algorithm.run(triangulation)?;

    // Display results
    info!("Simulation completed!");
    info!("Results:");
    info!("  Steps executed: {}", results.steps().len());
    info!(
        "  Acceptance rate: {:.2}%",
        results.acceptance_rate() * 100.0
    );
    info!("  Average action: {:.3}", results.average_action());

    // Final triangulation statistics
    let final_triangulation = results.triangulation();
    info!("Final triangulation:");
    info!("  Vertices: {}", final_triangulation.vertex_count());
    info!("  Edges: {}", final_triangulation.edge_count());
    info!("  Faces: {}", final_triangulation.face_count());

    // Display some measurements
    if !results.measurements().is_empty() {
        info!("Sample measurements:");
        for measurement in results.measurements().iter().take(5) {
            info!(
                "  Step {}: Action={:.3}, V={}, E={}, T={}",
                measurement.step,
                measurement.action,
                measurement.vertices,
                measurement.edges,
                measurement.triangles
            );
        }

        if results.measurements().len() > 5 {
            info!(
                "  ... ({} more measurements)",
                results.measurements().len() - 5
            );
        }
    }

    info!("Example completed successfully!");
    Ok(())
}
