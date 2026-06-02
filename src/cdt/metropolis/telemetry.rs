#![forbid(unsafe_code)]

//! Proposal and step telemetry for CDT Metropolis sampling.

use super::helpers::actions_match;
use crate::cdt::ergodic_moves::MoveType;
use crate::errors::{CdtError, CdtResult, CheckpointResumeFailure};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

/// Telemetry for one completed Monte Carlo step.
///
/// Step telemetry is emitted only for completed Metropolis transitions, so
/// [`Self::step`] is always nonzero. A step-0 construction or initial-state
/// sample appears as a [`Measurement`](crate::cdt::results::Measurement), not as
/// a `MonteCarloStep`.
///
/// Accepted, rejected-proposal, and no-proposal outcomes are stored as
/// [`MonteCarloStepOutcome`] variants, so accepted action payloads cannot be
/// partially present and rejected steps cannot carry an action-after value.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     ActionConfig, CdtResult, CdtTriangulation, MetropolisAlgorithm, MetropolisConfig,
/// };
///
/// fn main() -> CdtResult<()> {
///     let results = MetropolisAlgorithm::new(
///         MetropolisConfig::new(1.0, 1, 0, 1)?.with_seed(7),
///         ActionConfig::default(),
///     )
///     .run(CdtTriangulation::from_cdt_strip(4, 3)?)?;
///
///     let step = &results.steps()[0];
///     assert_eq!(step.step().get(), 1);
///     assert!(step.action_before().is_finite());
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct MonteCarloStep {
    step: NonZeroU32,
    move_type: MoveType,
    action_before: f64,
    outcome: MonteCarloStepOutcome,
}

#[derive(Deserialize)]
struct MonteCarloStepWire {
    step: NonZeroU32,
    move_type: MoveType,
    action_before: f64,
    outcome: MonteCarloStepOutcomeWire,
}

impl<'de> Deserialize<'de> for MonteCarloStep {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = MonteCarloStepWire::deserialize(deserializer)?;
        let outcome = MonteCarloStepOutcome::from_wire(wire.step, wire.action_before, wire.outcome)
            .map_err(DeError::custom)?;
        Self::new(wire.step, wire.move_type, wire.action_before, outcome).map_err(DeError::custom)
    }
}

impl MonteCarloStep {
    /// Creates validated telemetry for one completed Monte Carlo step.
    ///
    /// Use the outcome-specific constructors such as [`Self::accepted_step`] for
    /// the common public cases. This constructor is useful when code already has a
    /// validated [`MonteCarloStepOutcome`] from another boundary.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointResumeFailed`] when `action_before` is
    /// non-finite or when the supplied outcome carries non-finite or inconsistent
    /// action telemetry for this step.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MonteCarloStepOutcome, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome = MonteCarloStepOutcome::accepted_transition(
    ///         step_number,
    ///         4.0,
    ///         3.5,
    ///         -0.5,
    ///     )?;
    ///     let step = MonteCarloStep::new(step_number, MoveType::Move22, 4.0, outcome)?;
    ///
    ///     assert!(step.accepted());
    ///     assert_eq!(step.action_after(), Some(3.5));
    ///     Ok(())
    /// }
    /// ```
    pub fn new(
        step: NonZeroU32,
        move_type: MoveType,
        action_before: f64,
        outcome: MonteCarloStepOutcome,
    ) -> CdtResult<Self> {
        validate_action_before(step, action_before)?;
        outcome.validate_for_step(step, action_before)?;
        Ok(Self {
            step,
            move_type,
            action_before,
            outcome,
        })
    }

    /// Creates validated telemetry for an accepted Metropolis step.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointResumeFailed`] when any action value is
    /// non-finite or `action_after` does not match `action_before + delta_action`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::accepted_step(
    ///         step_number,
    ///         MoveType::Move22,
    ///         4.0,
    ///         3.5,
    ///         -0.5,
    ///     )?;
    ///
    ///     assert!(step.accepted());
    ///     assert_eq!(step.delta_action(), Some(-0.5));
    ///     Ok(())
    /// }
    /// ```
    pub fn accepted_step(
        step: NonZeroU32,
        move_type: MoveType,
        action_before: f64,
        action_after: f64,
        delta_action: f64,
    ) -> CdtResult<Self> {
        Self::new(
            step,
            move_type,
            action_before,
            MonteCarloStepOutcome::accepted_transition(
                step,
                action_before,
                action_after,
                delta_action,
            )?,
        )
    }

    /// Creates validated telemetry for a rejected step with a sampled proposal.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointResumeFailed`] when `action_before` or the
    /// optional proposal delta is non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::rejected_proposal(
    ///         step_number,
    ///         MoveType::Move13Add,
    ///         4.0,
    ///         Some(0.25),
    ///     )?;
    ///
    ///     assert!(!step.accepted());
    ///     assert_eq!(step.action_after(), None);
    ///     assert_eq!(step.delta_action(), Some(0.25));
    ///     Ok(())
    /// }
    /// ```
    pub fn rejected_proposal(
        step: NonZeroU32,
        move_type: MoveType,
        action_before: f64,
        delta_action: Option<f64>,
    ) -> CdtResult<Self> {
        Self::new(
            step,
            move_type,
            action_before,
            MonteCarloStepOutcome::rejected_proposal(step, delta_action)?,
        )
    }

    /// Creates validated telemetry for a selected move family with no local proposal.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointResumeFailed`] when `action_before` is
    /// non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::no_proposal(step_number, MoveType::EdgeFlip, 4.0)?;
    ///
    ///     assert!(!step.accepted());
    ///     assert_eq!(step.action_after(), None);
    ///     assert_eq!(step.delta_action(), None);
    ///     Ok(())
    /// }
    /// ```
    pub fn no_proposal(
        step: NonZeroU32,
        move_type: MoveType,
        action_before: f64,
    ) -> CdtResult<Self> {
        Self::new(
            step,
            move_type,
            action_before,
            MonteCarloStepOutcome::NoProposal,
        )
    }

    /// Returns the nonzero Monte Carlo step number.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 2, 0, 1)?.steps();
    ///     let step = MonteCarloStep::no_proposal(step_number, MoveType::EdgeFlip, 4.0)?;
    ///
    ///     assert_eq!(step.step().get(), 2);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn step(&self) -> NonZeroU32 {
        self.step
    }

    /// Returns the move type attempted during this step.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::no_proposal(step_number, MoveType::EdgeFlip, 4.0)?;
    ///
    ///     assert_eq!(step.move_type(), MoveType::EdgeFlip);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn move_type(&self) -> MoveType {
        self.move_type
    }

    /// Returns the action before the proposed move.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::no_proposal(step_number, MoveType::EdgeFlip, 4.0)?;
    ///
    ///     assert_eq!(step.action_before(), 4.0);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn action_before(&self) -> f64 {
        self.action_before
    }

    /// Returns the validated step outcome.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MonteCarloStepOutcome, MoveType,
    /// };
    /// use std::assert_matches;
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::no_proposal(step_number, MoveType::EdgeFlip, 4.0)?;
    ///
    ///     assert_matches!(step.outcome(), MonteCarloStepOutcome::NoProposal);
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn outcome(&self) -> &MonteCarloStepOutcome {
        &self.outcome
    }

    /// Returns whether the step was accepted by the Metropolis-Hastings transition.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::accepted_step(
    ///         step_number,
    ///         MoveType::Move22,
    ///         4.0,
    ///         3.5,
    ///         -0.5,
    ///     )?;
    ///
    ///     assert!(step.accepted());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn accepted(&self) -> bool {
        matches!(self.outcome, MonteCarloStepOutcome::Accepted(_))
    }

    /// Returns the action after the step when the proposal was accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::accepted_step(
    ///         step_number,
    ///         MoveType::Move22,
    ///         4.0,
    ///         3.5,
    ///         -0.5,
    ///     )?;
    ///
    ///     assert_eq!(step.action_after(), Some(3.5));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn action_after(&self) -> Option<f64> {
        self.outcome.action_after()
    }

    /// Returns the proposed or accepted action delta when available.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStep, MoveType,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let step = MonteCarloStep::rejected_proposal(
    ///         step_number,
    ///         MoveType::Move13Add,
    ///         4.0,
    ///         Some(0.25),
    ///     )?;
    ///
    ///     assert_eq!(step.delta_action(), Some(0.25));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn delta_action(&self) -> Option<f64> {
        self.outcome.delta_action()
    }
}

/// Action payload for an accepted Monte Carlo step.
///
/// This payload is present only in [`MonteCarloStepOutcome::Accepted`]. It keeps
/// `action_after` and `delta_action` together so accepted telemetry cannot store
/// one without the other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AcceptedStepTelemetry {
    action_after: f64,
    delta_action: f64,
}

impl AcceptedStepTelemetry {
    /// Returns the action after the accepted transition.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome =
    ///         MonteCarloStepOutcome::accepted_transition(step_number, 4.0, 3.5, -0.5)?;
    ///
    ///     if let MonteCarloStepOutcome::Accepted(payload) = outcome {
    ///         assert_eq!(payload.action_after(), 3.5);
    ///     }
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn action_after(self) -> f64 {
        self.action_after
    }

    /// Returns the accepted action delta.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome =
    ///         MonteCarloStepOutcome::accepted_transition(step_number, 4.0, 3.5, -0.5)?;
    ///
    ///     if let MonteCarloStepOutcome::Accepted(payload) = outcome {
    ///         assert_eq!(payload.delta_action(), -0.5);
    ///     }
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn delta_action(self) -> f64 {
        self.delta_action
    }
}

/// Action payload for a rejected concrete proposal.
///
/// Rejected proposals never carry an action-after value, but the proposal kernel
/// may still report the action delta for the rejected candidate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct RejectedProposalStepTelemetry {
    delta_action: Option<f64>,
}

impl RejectedProposalStepTelemetry {
    /// Returns the proposed action delta when the proposal kernel supplied one.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome = MonteCarloStepOutcome::rejected_proposal(step_number, Some(0.25))?;
    ///
    ///     if let MonteCarloStepOutcome::RejectedProposal(payload) = outcome {
    ///         assert_eq!(payload.delta_action(), Some(0.25));
    ///     }
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn delta_action(self) -> Option<f64> {
        self.delta_action
    }
}

/// Validated outcome for one completed Monte Carlo step.
///
/// The variants encode which action payloads are legal for the step outcome.
/// Accepted steps carry both an action-after value and a delta, rejected
/// proposals may carry only a candidate delta, and no-proposal steps carry no
/// action payload.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::{
///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
/// };
/// use std::assert_matches;
///
/// fn main() -> CdtResult<()> {
///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
///     let outcome =
///         MonteCarloStepOutcome::accepted_transition(step_number, 4.0, 3.5, -0.5)?;
///
///     assert_matches!(outcome, MonteCarloStepOutcome::Accepted(_));
///     if let MonteCarloStepOutcome::Accepted(payload) = outcome {
///         assert_eq!(payload.action_after(), 3.5);
///         assert_eq!(payload.delta_action(), -0.5);
///     }
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum MonteCarloStepOutcome {
    /// A proposal was accepted and committed to the CDT chain.
    Accepted(AcceptedStepTelemetry),
    /// A valid proposal was sampled but rejected by the Metropolis draw.
    RejectedProposal(RejectedProposalStepTelemetry),
    /// The selected move family had no sampleable local proposal.
    NoProposal,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum MonteCarloStepOutcomeWire {
    Accepted {
        action_after: f64,
        delta_action: f64,
    },
    RejectedProposal {
        delta_action: Option<f64>,
    },
    NoProposal,
}

impl MonteCarloStepOutcome {
    /// Creates a validated accepted-step outcome.
    ///
    /// Use this when a boundary already has the move-independent action telemetry
    /// and needs an invariant-bearing outcome before constructing a
    /// [`MonteCarloStep`].
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointResumeFailed`] when either action value is
    /// non-finite or `action_after` does not match `action_before + delta_action`.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome =
    ///         MonteCarloStepOutcome::accepted_transition(step_number, 4.0, 3.5, -0.5)?;
    ///
    ///     assert!(outcome.accepted());
    ///     assert_eq!(outcome.action_after(), Some(3.5));
    ///     Ok(())
    /// }
    /// ```
    pub fn accepted_transition(
        step: NonZeroU32,
        action_before: f64,
        action_after: f64,
        delta_action: f64,
    ) -> CdtResult<Self> {
        validate_action_after(step, action_after)?;
        validate_delta_action(step, delta_action)?;
        if !actions_match(action_after, action_before + delta_action) {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::StepActionAfterDeltaMismatch { step: step.get() },
            ));
        }
        Ok(Self::Accepted(AcceptedStepTelemetry {
            action_after,
            delta_action,
        }))
    }

    /// Creates a validated rejected-proposal outcome.
    ///
    /// Use this for a concrete proposal that was sampled and rejected by the
    /// Metropolis draw. Use [`Self::NoProposal`] when no local candidate was
    /// available.
    ///
    /// # Errors
    ///
    /// Returns [`CdtError::CheckpointResumeFailed`] when the optional proposal
    /// delta is non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome = MonteCarloStepOutcome::rejected_proposal(step_number, Some(0.5))?;
    ///
    ///     assert!(!outcome.accepted());
    ///     assert_eq!(outcome.delta_action(), Some(0.5));
    ///     Ok(())
    /// }
    /// ```
    pub fn rejected_proposal(step: NonZeroU32, delta_action: Option<f64>) -> CdtResult<Self> {
        if let Some(delta_action) = delta_action {
            validate_delta_action(step, delta_action)?;
        }
        Ok(Self::RejectedProposal(RejectedProposalStepTelemetry {
            delta_action,
        }))
    }

    /// Returns whether this outcome accepted the proposed transition.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome =
    ///         MonteCarloStepOutcome::accepted_transition(step_number, 4.0, 3.5, -0.5)?;
    ///
    ///     assert!(outcome.accepted());
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn accepted(self) -> bool {
        matches!(self, Self::Accepted(_))
    }

    /// Returns the action after the step when this is an accepted outcome.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome =
    ///         MonteCarloStepOutcome::accepted_transition(step_number, 4.0, 3.5, -0.5)?;
    ///
    ///     assert_eq!(outcome.action_after(), Some(3.5));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn action_after(self) -> Option<f64> {
        match self {
            Self::Accepted(payload) => Some(payload.action_after()),
            Self::RejectedProposal(_) | Self::NoProposal => None,
        }
    }

    /// Returns the proposal or accepted action delta when available.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::{
    ///     CdtResult, MetropolisConfig, MonteCarloStepOutcome,
    /// };
    ///
    /// fn main() -> CdtResult<()> {
    ///     let step_number = MetropolisConfig::new(1.0, 1, 0, 1)?.steps();
    ///     let outcome = MonteCarloStepOutcome::rejected_proposal(step_number, Some(0.25))?;
    ///
    ///     assert_eq!(outcome.delta_action(), Some(0.25));
    ///     Ok(())
    /// }
    /// ```
    #[must_use]
    pub const fn delta_action(self) -> Option<f64> {
        match self {
            Self::Accepted(payload) => Some(payload.delta_action()),
            Self::RejectedProposal(payload) => payload.delta_action(),
            Self::NoProposal => None,
        }
    }

    /// Re-validates an outcome with the step-local action-before context.
    ///
    /// Public constructors call this before storing caller-supplied outcomes so
    /// deserialized or externally assembled telemetry cannot bypass the accepted
    /// action-after/delta consistency contract.
    fn validate_for_step(self, step: NonZeroU32, action_before: f64) -> CdtResult<()> {
        match self {
            Self::Accepted(payload) => {
                Self::accepted_transition(
                    step,
                    action_before,
                    payload.action_after(),
                    payload.delta_action(),
                )?;
            }
            Self::RejectedProposal(payload) => {
                Self::rejected_proposal(step, payload.delta_action())?;
            }
            Self::NoProposal => {}
        }
        Ok(())
    }

    /// Converts the raw serialized outcome shape into validated domain telemetry.
    ///
    /// The wire payload is intentionally private because finite-action and
    /// accepted-step delta consistency checks need the enclosing step number and
    /// action-before value for precise diagnostics.
    fn from_wire(
        step: NonZeroU32,
        action_before: f64,
        wire: MonteCarloStepOutcomeWire,
    ) -> CdtResult<Self> {
        match wire {
            MonteCarloStepOutcomeWire::Accepted {
                action_after,
                delta_action,
            } => Self::accepted_transition(step, action_before, action_after, delta_action),
            MonteCarloStepOutcomeWire::RejectedProposal { delta_action } => {
                Self::rejected_proposal(step, delta_action)
            }
            MonteCarloStepOutcomeWire::NoProposal => Ok(Self::NoProposal),
        }
    }
}

const fn checkpoint_resume_failed(failure: CheckpointResumeFailure) -> CdtError {
    CdtError::CheckpointResumeFailed { failure }
}

const fn validate_action_before(step: NonZeroU32, action_before: f64) -> CdtResult<()> {
    if action_before.is_finite() {
        Ok(())
    } else {
        Err(checkpoint_resume_failed(
            CheckpointResumeFailure::NonFiniteStepActionBefore { step: step.get() },
        ))
    }
}

const fn validate_action_after(step: NonZeroU32, action_after: f64) -> CdtResult<()> {
    if action_after.is_finite() {
        Ok(())
    } else {
        Err(checkpoint_resume_failed(
            CheckpointResumeFailure::NonFiniteStepActionAfter { step: step.get() },
        ))
    }
}

const fn validate_delta_action(step: NonZeroU32, delta_action: f64) -> CdtResult<()> {
    if delta_action.is_finite() {
        Ok(())
    } else {
        Err(checkpoint_resume_failed(
            CheckpointResumeFailure::NonFiniteStepDeltaAction { step: step.get() },
        ))
    }
}

/// Local-site rejection observed while trying to realize an accepted CDT proposal.
///
/// These rejections mean the move type was selected and accepted at the
/// count-action level, but the bounded random local-site search did not find a
/// concrete site where the move could be applied.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CdtProposalSiteRejection {
    /// The selected local site would violate CDT causality.
    CausalityViolation,
    /// The selected local site was geometrically invalid.
    GeometricViolation,
    /// The selected local site was rejected by the backend mutation kernel.
    Kernel(CdtError),
}

impl fmt::Display for CdtProposalSiteRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CausalityViolation => {
                f.write_str("causality violation at selected application site")
            }
            Self::GeometricViolation => {
                f.write_str("geometric violation at selected application site")
            }
            Self::Kernel(err) => err.fmt(f),
        }
    }
}

impl Error for CdtProposalSiteRejection {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Kernel(err) => Some(err),
            Self::CausalityViolation | Self::GeometricViolation => None,
        }
    }
}

/// Telemetry for concrete Metropolis proposal outcomes.
///
/// These counters describe the proposal kernel observed during a run. They are
/// diagnostic only: detailed balance is enforced by the per-step proposal
/// probability used in the Hastings ratio, not by accumulated empirical counts.
///
/// The struct is non-exhaustive so future releases can add proposal telemetry
/// without breaking downstream code. Construct empty accumulators with
/// [`Self::new`] or [`Default::default`], and inspect fields through shared
/// references returned by
/// [`checkpoint proposal stats`][super::CdtMcmcCheckpoint::proposal_stats] or
/// [`result proposal stats`][crate::cdt::results::SimulationResultsBackend::proposal_stats].
/// Deserialization requires exactly one terminal outcome for every selected
/// move-family proposal, and counters saturate at `u64::MAX` instead of
/// wrapping.
///
/// # Examples
///
/// ```
/// use causal_triangulations::prelude::simulation::ProposalStatistics;
///
/// let stats = ProposalStatistics::new();
/// assert_eq!(stats.move_family_proposals(), 0);
/// assert_eq!(stats.accepted_transitions(), 0);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ProposalStatistics {
    /// Number of selected move-family proposals, saturating at `u64::MAX`.
    move_family_proposals: u64,
    /// Sum of sampleable forward-site denominators observed during planning,
    /// saturating at `u64::MAX`.
    observed_forward_sites: u64,
    /// Number of proposals with no sampleable local site, saturating at `u64::MAX`.
    no_site_proposals: u64,
    /// Number of sampled sites rejected by causal checks, saturating at `u64::MAX`.
    site_causality_rejections: u64,
    /// Number of sampled sites rejected by geometric checks, saturating at `u64::MAX`.
    site_geometric_rejections: u64,
    /// Number of sampled sites rejected by backend mutation errors, saturating at `u64::MAX`.
    site_backend_rejections: u64,
    /// Number of valid proposed transitions rejected by the Metropolis draw,
    /// saturating at `u64::MAX`.
    metropolis_rejections: u64,
    /// Number of proposed transitions committed to the chain, saturating at `u64::MAX`.
    accepted_transitions: u64,
    /// Number of proposal attempts that hit a hard failure, saturating at `u64::MAX`.
    hard_failures: u64,
}

#[derive(Deserialize)]
struct ProposalStatisticsWire {
    move_family_proposals: u64,
    observed_forward_sites: u64,
    no_site_proposals: u64,
    site_causality_rejections: u64,
    site_geometric_rejections: u64,
    site_backend_rejections: u64,
    metropolis_rejections: u64,
    accepted_transitions: u64,
    hard_failures: u64,
}

impl<'de> Deserialize<'de> for ProposalStatistics {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ProposalStatisticsWire::deserialize(deserializer)?;
        Self::from_wire(&wire).map_err(DeError::custom)
    }
}

impl ProposalStatistics {
    /// Creates an empty proposal telemetry accumulator.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats, ProposalStatistics::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            move_family_proposals: 0,
            observed_forward_sites: 0,
            no_site_proposals: 0,
            site_causality_rejections: 0,
            site_geometric_rejections: 0,
            site_backend_rejections: 0,
            metropolis_rejections: 0,
            accepted_transitions: 0,
            hard_failures: 0,
        }
    }

    #[cfg(test)]
    #[expect(
        clippy::too_many_arguments,
        reason = "test and serde helpers need to preserve the flat telemetry wire shape"
    )]
    pub(crate) const fn from_validated_parts(
        move_family_proposals: u64,
        observed_forward_sites: u64,
        no_site_proposals: u64,
        site_causality_rejections: u64,
        site_geometric_rejections: u64,
        site_backend_rejections: u64,
        metropolis_rejections: u64,
        accepted_transitions: u64,
        hard_failures: u64,
    ) -> Self {
        Self {
            move_family_proposals,
            observed_forward_sites,
            no_site_proposals,
            site_causality_rejections,
            site_geometric_rejections,
            site_backend_rejections,
            metropolis_rejections,
            accepted_transitions,
            hard_failures,
        }
    }

    /// Rebuilds proposal telemetry from the serialized wire shape.
    ///
    /// The wire form is rejected when terminal outcomes cannot be summed without
    /// overflow, do not exactly account for selected move families, or when
    /// forward-site observations exist without any selected move family. That
    /// keeps deserialized result and checkpoint telemetry coherent before public
    /// accessors expose the counters.
    fn from_wire(wire: &ProposalStatisticsWire) -> CdtResult<Self> {
        let terminal_outcomes = [
            wire.no_site_proposals,
            wire.site_causality_rejections,
            wire.site_geometric_rejections,
            wire.site_backend_rejections,
            wire.metropolis_rejections,
            wire.accepted_transitions,
            wire.hard_failures,
        ]
        .into_iter()
        .try_fold(0_u64, |total, count| {
            total.checked_add(count).ok_or_else(|| {
                checkpoint_resume_failed(CheckpointResumeFailure::ProposalTerminalCounterOverflow)
            })
        })?;
        if terminal_outcomes != wire.move_family_proposals {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::ProposalTerminalOutcomeCountMismatch {
                    terminal_outcomes,
                    move_family_proposals: wire.move_family_proposals,
                },
            ));
        }
        if wire.move_family_proposals == 0 && wire.observed_forward_sites != 0 {
            return Err(checkpoint_resume_failed(
                CheckpointResumeFailure::ProposalForwardSitesWithoutMoveFamily {
                    observed_forward_sites: wire.observed_forward_sites,
                },
            ));
        }
        Ok(Self {
            move_family_proposals: wire.move_family_proposals,
            observed_forward_sites: wire.observed_forward_sites,
            no_site_proposals: wire.no_site_proposals,
            site_causality_rejections: wire.site_causality_rejections,
            site_geometric_rejections: wire.site_geometric_rejections,
            site_backend_rejections: wire.site_backend_rejections,
            metropolis_rejections: wire.metropolis_rejections,
            accepted_transitions: wire.accepted_transitions,
            hard_failures: wire.hard_failures,
        })
    }

    /// Returns the number of selected move families.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.move_family_proposals(), 0);
    /// ```
    #[must_use]
    pub const fn move_family_proposals(&self) -> u64 {
        self.move_family_proposals
    }

    /// Returns the accumulated sampleable forward-site denominators.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.observed_forward_sites(), 0);
    /// ```
    #[must_use]
    pub const fn observed_forward_sites(&self) -> u64 {
        self.observed_forward_sites
    }

    /// Returns the number of proposals with no local site.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.no_site_proposals(), 0);
    /// ```
    #[must_use]
    pub const fn no_site_proposals(&self) -> u64 {
        self.no_site_proposals
    }

    /// Returns the number of sampled sites rejected by causality checks.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.site_causality_rejections(), 0);
    /// ```
    #[must_use]
    pub const fn site_causality_rejections(&self) -> u64 {
        self.site_causality_rejections
    }

    /// Returns the number of sampled sites rejected by geometric checks.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.site_geometric_rejections(), 0);
    /// ```
    #[must_use]
    pub const fn site_geometric_rejections(&self) -> u64 {
        self.site_geometric_rejections
    }

    /// Returns the number of sampled sites rejected by backend mutation errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.site_backend_rejections(), 0);
    /// ```
    #[must_use]
    pub const fn site_backend_rejections(&self) -> u64 {
        self.site_backend_rejections
    }

    /// Returns the number of valid transitions rejected by Metropolis.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.metropolis_rejections(), 0);
    /// ```
    #[must_use]
    pub const fn metropolis_rejections(&self) -> u64 {
        self.metropolis_rejections
    }

    /// Returns the number of committed transitions.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.accepted_transitions(), 0);
    /// ```
    #[must_use]
    pub const fn accepted_transitions(&self) -> u64 {
        self.accepted_transitions
    }

    /// Returns the number of hard proposal failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.hard_failures(), 0);
    /// ```
    #[must_use]
    pub const fn hard_failures(&self) -> u64 {
        self.hard_failures
    }

    /// Returns proposal outcomes that rejected a selected move family.
    ///
    /// This includes no-site, causality, geometric, backend, and Metropolis
    /// rejections. Accepted transitions and hard failures are intentionally
    /// reported by separate counters.
    ///
    /// # Examples
    ///
    /// ```
    /// use causal_triangulations::prelude::simulation::ProposalStatistics;
    ///
    /// let stats = ProposalStatistics::new();
    /// assert_eq!(stats.rejected_transitions(), 0);
    /// ```
    #[must_use]
    pub const fn rejected_transitions(&self) -> u64 {
        self.no_site_proposals
            .saturating_add(self.site_causality_rejections)
            .saturating_add(self.site_geometric_rejections)
            .saturating_add(self.site_backend_rejections)
            .saturating_add(self.metropolis_rejections)
    }

    /// Records one selected move family and the forward-site denominator observed for it.
    ///
    /// This is proposal-kernel telemetry only; the per-step Hastings ratio uses
    /// the instantaneous site count directly rather than accumulated statistics.
    pub(crate) fn record_move_family(&mut self, forward_sites: usize) {
        self.move_family_proposals = self.move_family_proposals.saturating_add(1);
        self.observed_forward_sites = self
            .observed_forward_sites
            .saturating_add(u64::try_from(forward_sites).unwrap_or(u64::MAX));
    }

    /// Records a move-family proposal with no concrete local site.
    pub(crate) const fn record_no_site(&mut self) {
        self.no_site_proposals = self.no_site_proposals.saturating_add(1);
    }

    /// Classifies a sampled-site rejection without changing chain state.
    ///
    /// These are ordinary self-loop proposal outcomes, not hard failures.
    pub(crate) const fn record_site_rejection(&mut self, rejection: &CdtProposalSiteRejection) {
        match rejection {
            CdtProposalSiteRejection::CausalityViolation => {
                self.site_causality_rejections = self.site_causality_rejections.saturating_add(1);
            }
            CdtProposalSiteRejection::GeometricViolation => {
                self.site_geometric_rejections = self.site_geometric_rejections.saturating_add(1);
            }
            CdtProposalSiteRejection::Kernel(_) => {
                self.site_backend_rejections = self.site_backend_rejections.saturating_add(1);
            }
        }
    }

    /// Records Metropolis rejection after a valid proposed transition was scored.
    pub(crate) const fn record_metropolis_rejection(&mut self) {
        self.metropolis_rejections = self.metropolis_rejections.saturating_add(1);
    }

    /// Records a proposed transition that was committed to the live chain.
    pub(crate) const fn record_accepted_transition(&mut self) {
        self.accepted_transitions = self.accepted_transitions.saturating_add(1);
    }

    /// Records an unexpected hard failure during proposal application.
    pub(crate) const fn record_hard_failure(&mut self) {
        self.hard_failures = self.hard_failures.saturating_add(1);
    }

    /// Adds another proposal-telemetry snapshot into this accumulator.
    ///
    /// Chunked Metropolis continuation merges per-step telemetry from the
    /// upstream planned-proposal sampler into CDT-owned counters. All additions
    /// saturate at `u64::MAX`, so already-saturated checkpoint telemetry remains
    /// serializable. Once any counter saturates, the merged totals may no longer
    /// preserve an exact one-to-one terminal-outcome partition; saturation can
    /// coarsen the precise accepted, rejected, and hard-failure split.
    pub(crate) const fn extend(&mut self, other: &Self) {
        self.move_family_proposals = self
            .move_family_proposals
            .saturating_add(other.move_family_proposals);
        self.observed_forward_sites = self
            .observed_forward_sites
            .saturating_add(other.observed_forward_sites);
        self.no_site_proposals = self
            .no_site_proposals
            .saturating_add(other.no_site_proposals);
        self.site_causality_rejections = self
            .site_causality_rejections
            .saturating_add(other.site_causality_rejections);
        self.site_geometric_rejections = self
            .site_geometric_rejections
            .saturating_add(other.site_geometric_rejections);
        self.site_backend_rejections = self
            .site_backend_rejections
            .saturating_add(other.site_backend_rejections);
        self.metropolis_rejections = self
            .metropolis_rejections
            .saturating_add(other.metropolis_rejections);
        self.accepted_transitions = self
            .accepted_transitions
            .saturating_add(other.accepted_transitions);
        self.hard_failures = self.hard_failures.saturating_add(other.hard_failures);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    fn step_number(step: u32) -> NonZeroU32 {
        NonZeroU32::new(step).expect("test step number should be nonzero")
    }

    fn assert_checkpoint_failure(
        error: CdtError,
        matches_failure: impl FnOnce(&CheckpointResumeFailure) -> bool,
    ) {
        match error {
            CdtError::CheckpointResumeFailed { failure } => assert!(
                matches_failure(&failure),
                "unexpected checkpoint failure: {failure:?}"
            ),
            other => panic!("expected checkpoint resume failure, got {other:?}"),
        }
    }

    fn assert_optional_actions_match(actual: Option<f64>, expected: Option<f64>) {
        match (actual, expected) {
            (Some(actual), Some(expected)) => assert!(
                actions_match(actual, expected),
                "expected {actual} to match {expected}"
            ),
            (None, None) => {}
            (actual, expected) => panic!("expected {actual:?} to match {expected:?}"),
        }
    }

    #[test]
    fn monte_carlo_step_new_revalidates_outcome_against_action_before() {
        let outcome = MonteCarloStepOutcome::accepted_transition(step_number(1), 4.0, 3.5, -0.5)
            .expect("test outcome should satisfy its original action context");

        let error = MonteCarloStep::new(step_number(1), MoveType::Move22, 10.0, outcome)
            .expect_err("step constructor should reject outcome inconsistent with action_before");

        assert_checkpoint_failure(error, |failure| {
            matches!(
                failure,
                CheckpointResumeFailure::StepActionAfterDeltaMismatch { step: 1 }
            )
        });
    }

    #[test]
    fn monte_carlo_step_serde_round_trips_valid_outcome_variants() {
        let steps = [
            MonteCarloStep::accepted_step(step_number(1), MoveType::Move22, 4.0, 3.5, -0.5)
                .expect("test accepted step should satisfy action invariants"),
            MonteCarloStep::rejected_proposal(step_number(2), MoveType::Move13Add, 3.5, Some(0.25))
                .expect("test rejected-proposal step should satisfy action invariants"),
            MonteCarloStep::no_proposal(step_number(3), MoveType::EdgeFlip, 3.5)
                .expect("test no-proposal step should satisfy action invariants"),
        ];

        for step in steps {
            let value = serde_json::to_value(&step).expect("step telemetry should serialize");
            let round_tripped: MonteCarloStep =
                serde_json::from_value(value).expect("valid step telemetry should deserialize");

            assert_eq!(round_tripped.step(), step.step());
            assert_eq!(round_tripped.move_type(), step.move_type());
            assert!(
                actions_match(round_tripped.action_before(), step.action_before()),
                "round-tripped action_before should match original"
            );
            assert_eq!(round_tripped.accepted(), step.accepted());
            assert_optional_actions_match(round_tripped.action_after(), step.action_after());
            assert_optional_actions_match(round_tripped.delta_action(), step.delta_action());
        }
    }

    #[test]
    fn monte_carlo_step_deserialization_rejects_accepted_delta_mismatch() {
        let payload = r#"{
            "step": 1,
            "move_type": "Move22",
            "action_before": 4.0,
            "outcome": {
                "Accepted": {
                    "action_after": 3.5,
                    "delta_action": 0.0
                }
            }
        }"#;

        let error = serde_json::from_str::<MonteCarloStep>(payload)
            .expect_err("accepted action-after/delta mismatch should be rejected");

        assert!(
            error
                .to_string()
                .contains("action_after does not match delta_action"),
            "serde error should explain accepted-step action invariant, got {error}"
        );
    }

    #[test]
    fn monte_carlo_step_deserialization_preserves_rejected_proposal_kind() {
        let payload = r#"{
            "step": 2,
            "move_type": "Move13Add",
            "action_before": 3.5,
            "outcome": {
                "RejectedProposal": {
                    "delta_action": null
                }
            }
        }"#;

        let step = serde_json::from_str::<MonteCarloStep>(payload)
            .expect("rejected proposal without delta should deserialize");

        assert_eq!(step.step().get(), 2);
        assert_eq!(step.move_type(), MoveType::Move13Add);
        assert_matches!(
            step.outcome(),
            MonteCarloStepOutcome::RejectedProposal(payload) if payload.delta_action().is_none()
        );
        assert!(!step.accepted());
        assert_eq!(step.action_after(), None);
        assert_eq!(step.delta_action(), None);
    }

    #[test]
    fn proposal_statistics_from_wire_rejects_terminal_outcomes_above_proposals() {
        let wire = ProposalStatisticsWire {
            move_family_proposals: 1,
            observed_forward_sites: 1,
            no_site_proposals: 1,
            site_causality_rejections: 0,
            site_geometric_rejections: 0,
            site_backend_rejections: 0,
            metropolis_rejections: 0,
            accepted_transitions: 1,
            hard_failures: 0,
        };

        let error = ProposalStatistics::from_wire(&wire)
            .expect_err("terminal outcomes above move-family proposals should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalTerminalOutcomeCountMismatch {
                    terminal_outcomes: 2,
                    move_family_proposals: 1,
                }
            }
        );
    }

    #[test]
    fn proposal_statistics_from_wire_rejects_under_classified_proposals() {
        let wire = ProposalStatisticsWire {
            move_family_proposals: 2,
            observed_forward_sites: 1,
            no_site_proposals: 1,
            site_causality_rejections: 0,
            site_geometric_rejections: 0,
            site_backend_rejections: 0,
            metropolis_rejections: 0,
            accepted_transitions: 0,
            hard_failures: 0,
        };

        let error = ProposalStatistics::from_wire(&wire)
            .expect_err("under-classified move-family proposals should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalTerminalOutcomeCountMismatch {
                    terminal_outcomes: 1,
                    move_family_proposals: 2,
                }
            }
        );
    }

    #[test]
    fn proposal_statistics_from_wire_rejects_forward_sites_without_proposals() {
        let wire = ProposalStatisticsWire {
            move_family_proposals: 0,
            observed_forward_sites: 1,
            no_site_proposals: 0,
            site_causality_rejections: 0,
            site_geometric_rejections: 0,
            site_backend_rejections: 0,
            metropolis_rejections: 0,
            accepted_transitions: 0,
            hard_failures: 0,
        };

        let error = ProposalStatistics::from_wire(&wire)
            .expect_err("forward sites without proposals should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalForwardSitesWithoutMoveFamily {
                    observed_forward_sites: 1
                }
            }
        );
    }

    #[test]
    fn proposal_statistics_from_wire_rejects_terminal_outcome_counter_overflow() {
        let wire = ProposalStatisticsWire {
            move_family_proposals: u64::MAX,
            observed_forward_sites: u64::MAX,
            no_site_proposals: u64::MAX,
            site_causality_rejections: 1,
            site_geometric_rejections: 0,
            site_backend_rejections: 0,
            metropolis_rejections: 0,
            accepted_transitions: 0,
            hard_failures: 0,
        };

        let error = ProposalStatistics::from_wire(&wire)
            .expect_err("overflowed terminal outcome partition should be rejected");

        assert_matches!(
            error,
            CdtError::CheckpointResumeFailed {
                failure: CheckpointResumeFailure::ProposalTerminalCounterOverflow
            }
        );
    }
}
