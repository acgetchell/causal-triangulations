#![forbid(unsafe_code)]

//! Example: measuring CDT slab-triangle profiles and effective dimensional observables.
//!
//! This example builds an explicit foliated toroidal CDT, records per-slice
//! triangle counts, runs a short Metropolis simulation, and computes aggregate
//! slab-profile and finite-graph effective dimensional observables.

use causal_triangulations::prelude::errors::CdtResult;
use causal_triangulations::prelude::observables::*;
use causal_triangulations::prelude::simulation::{
    ActionConfig, MetropolisAlgorithm, MetropolisConfig,
};

fn main() -> CdtResult<()> {
    let triangulation = CdtTriangulation::from_toroidal_cdt(8, 8)?;

    let initial_profile = triangulation.slab_triangle_profile()?;
    println!("Initial slab-triangle profile N2(t): {initial_profile:?}");

    let hausdorff = estimate_all_scale_effective_hausdorff_slope(&triangulation)?.map_or_else(
        || "not enough dual-graph data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Initial all-scale effective Hausdorff slope: {hausdorff}");

    let spectral = estimate_short_time_effective_spectral_dimension(&triangulation)?.map_or_else(
        || "not enough dual-graph diffusion data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Initial short-time effective spectral dimension: {spectral}");

    let metropolis_config = MetropolisConfig::new(1.0, 80, 20, 10)?.with_seed(7);
    let action_config = ActionConfig::default();
    let results = MetropolisAlgorithm::new(metropolis_config, action_config).run(triangulation)?;

    println!(
        "Average post-thermalization slab-triangle profile: {:?}",
        results.average_slab_triangle_profile()
    );
    println!(
        "Post-thermalization slab-triangle fluctuations: {:?}",
        results.slab_triangle_fluctuations()
    );

    let final_hausdorff = results.all_scale_effective_hausdorff_slope()?.map_or_else(
        || "not enough dual-graph data".to_string(),
        |dimension| format!("{dimension:.3}"),
    );
    println!("Final all-scale effective Hausdorff slope: {final_hausdorff}");

    let final_spectral = results
        .short_time_effective_spectral_dimension()?
        .map_or_else(
            || "not enough dual-graph diffusion data".to_string(),
            |dimension| format!("{dimension:.3}"),
        );
    println!("Final short-time effective spectral dimension: {final_spectral}");

    Ok(())
}
