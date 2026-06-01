#![forbid(unsafe_code)]

//! Example: measuring CDT volume profiles and dimensional observables.
//!
//! This example builds an explicit foliated toroidal CDT, records per-slice
//! triangle counts, runs a short Metropolis simulation, and computes aggregate
//! volume-profile, Hausdorff-dimension, and spectral-dimension observables.

use causal_triangulations::prelude::errors::CdtResult;
use causal_triangulations::prelude::observables::*;
use causal_triangulations::prelude::simulation::{
    ActionConfig, MetropolisAlgorithm, MetropolisConfig,
};

fn main() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(8, 8)?;

    let initial_profile = triangulation.volume_profile();
    println!("Initial volume profile N2(t): {initial_profile:?}");

    let hausdorff = estimate_hausdorff_dimension(&triangulation).map_or_else(
        || "not enough dual-graph data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Initial Hausdorff-dimension estimate: {hausdorff}");

    let spectral = estimate_spectral_dimension(&triangulation).map_or_else(
        || "not enough dual-graph diffusion data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Initial spectral-dimension estimate: {spectral}");

    let metropolis_config = MetropolisConfig::new(1.0, 80, 20, 10)?.with_seed(7);
    let action_config = ActionConfig::default();
    let results = MetropolisAlgorithm::new(metropolis_config, action_config).run(triangulation)?;

    println!(
        "Average post-thermalization volume profile: {:?}",
        results.average_volume_profile()
    );
    println!(
        "Post-thermalization volume fluctuations: {:?}",
        results.volume_fluctuations()
    );

    let final_hausdorff = results.hausdorff_dimension_estimate().map_or_else(
        || "not enough dual-graph data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Final Hausdorff-dimension estimate: {final_hausdorff}");

    let final_spectral = results.spectral_dimension_estimate().map_or_else(
        || "not enough dual-graph diffusion data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Final spectral-dimension estimate: {final_spectral}");

    Ok(())
}
