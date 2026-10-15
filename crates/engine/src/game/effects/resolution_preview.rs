//! Read-only preview of the chain links a resolving spell or ability will
//! follow, given only what is already fixed when it is put on the stack.
//!
//! CR 608.2c: the controller follows the instructions in the order written,
//! and later text may modify earlier text — a conditional link runs only when
//! its condition holds, and an "instead" link replaces the instruction before
//! it (a self-replacement effect, CR 614.15). Some of those conditions are
//! settled before resolution begins: whether an additional or alternative cost
//! was paid (CR 601.2b + CR 601.2h, e.g. "if this spell was kicked", CR
//! 702.33d), where and when the spell was cast, and how it was paid for. Others
//! depend on the game state at resolution or on an earlier instruction's
//! outcome. This module evaluates the first kind against the actual resolved
//! context and stops at the second kind, so a caller (the AI's threat reading,
//! for one) can rely on every link it reports without guessing at the rest.
//!
//! The walk mirrors `resolve_chain_body`'s dispatch order — the "instead" swap
//! first, then the head's own condition, then its else branch or its
//! sub-ability — and reuses the same authorities: [`evaluate_condition`] for
//! every condition it treats as known, [`apply_instead_swap`] for the swap,
//! [`should_propagate_parent_targets`] plus `mirror_targets_from` for target
//! inheritance, and the state-free half of the parent->child context hand-off.

use crate::game::ability_utils::apply_instead_swap;
use crate::types::ability::{
    AbilityCondition, Effect, ObjectScope, ResolvedAbility, SubAbilityLink, TargetChoiceTiming,
    TargetSelectionMode,
};
use crate::types::game_state::GameState;

use super::{evaluate_condition, inherit_parent_chain_context, should_propagate_parent_targets};

/// Whether a condition's value is already settled before resolution.
///
/// Three-valued (Kleene) logic: a compound condition is settled as soon as a
/// settled operand decides it, even when another operand is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionOutcome {
    /// Settled true: the link this condition gates will be followed.
    Holds,
    /// Settled false: the link this condition gates will be skipped.
    Fails,
    /// Depends on the game state at resolution, or on an earlier
    /// instruction's outcome; nothing may be concluded from it now.
    Undetermined,
}

impl ConditionOutcome {
    fn from_settled(holds: bool) -> Self {
        if holds {
            Self::Holds
        } else {
            Self::Fails
        }
    }

    /// Kleene negation: an undetermined value stays undetermined.
    pub fn negate(self) -> Self {
        match self {
            Self::Holds => Self::Fails,
            Self::Fails => Self::Holds,
            Self::Undetermined => Self::Undetermined,
        }
    }

    /// Kleene conjunction: any settled-false operand settles it false.
    pub fn all(outcomes: impl IntoIterator<Item = Self>) -> Self {
        outcomes
            .into_iter()
            .fold(Self::Holds, |acc, outcome| match (acc, outcome) {
                (Self::Fails, _) | (_, Self::Fails) => Self::Fails,
                (Self::Undetermined, _) | (_, Self::Undetermined) => Self::Undetermined,
                (Self::Holds, Self::Holds) => Self::Holds,
            })
    }

    /// Kleene disjunction: any settled-true operand settles it true.
    pub fn any(outcomes: impl IntoIterator<Item = Self>) -> Self {
        outcomes
            .into_iter()
            .fold(Self::Fails, |acc, outcome| match (acc, outcome) {
                (Self::Holds, _) | (_, Self::Holds) => Self::Holds,
                (Self::Undetermined, _) | (_, Self::Undetermined) => Self::Undetermined,
                (Self::Fails, Self::Fails) => Self::Fails,
            })
    }
}

/// CR 608.2c: whether `condition`, evaluated for `ability`, is already settled.
///
/// Settled conditions are read through [`evaluate_condition`], the resolver's
/// own authority, so the preview can never disagree with resolution about
/// them. A condition is settled only when it reads a fact fixed by how the
/// spell or ability was put on the stack — a cost choice recorded in its
/// `SpellContext` or a cast stamp on its own object. Everything that reads the
/// game state at resolution, an earlier instruction's outcome, or a trigger
/// event is [`ConditionOutcome::Undetermined`].
pub fn known_condition_outcome(
    condition: &AbilityCondition,
    state: &GameState,
    ability: &ResolvedAbility,
) -> ConditionOutcome {
    let settled = || ConditionOutcome::from_settled(evaluate_condition(condition, state, ability));
    // CR 400.7: cast stamps live on the spell's own object. Without that
    // object there is nothing settled to read.
    let settled_on_source = || {
        if state.objects.contains_key(&ability.source_id) {
            settled()
        } else {
            ConditionOutcome::Undetermined
        }
    };
    match condition {
        // CR 601.2b + CR 601.2h + CR 702.33d: an optional additional cost —
        // kicker, gift, buyback, teamwork — is chosen and paid while casting
        // and recorded in the resolving ability's own `SpellContext`.
        AbilityCondition::AdditionalCostPaid { subject, .. } => match subject {
            ObjectScope::Source | ObjectScope::Anaphoric | ObjectScope::Demonstrative => settled(),
            // The first object target's payments: the target may yet change
            // or leave its zone before resolution.
            ObjectScope::Target
            | ObjectScope::Recipient
            | ObjectScope::EventSource
            | ObjectScope::CostPaidObject
            | ObjectScope::OtherRevealedCard
            | ObjectScope::OwnedLinkedExileCard
            | ObjectScope::EventTarget
            | ObjectScope::AmassedArmy
            | ObjectScope::ChainRootTarget
            | ObjectScope::GrantingObject
            | ObjectScope::SpecificObject { .. }
            | ObjectScope::BatchSource => ConditionOutcome::Undetermined,
        },
        // CR 614.15 + CR 702.33d: "if this spell was kicked, … instead" reads
        // the same recorded payment.
        AbilityCondition::AdditionalCostPaidInstead
        // CR 118.9: an alternative mana cost is chosen while casting.
        | AbilityCondition::AlternativeManaCostPaid
        // CR 601.2a: the zone a spell was cast from, and the phase it was
        // cast in, are recorded when it is cast.
        | AbilityCondition::WasCast { .. }
        | AbilityCondition::CastDuringPhase { .. } => settled(),
        AbilityCondition::CastVariantPaid { subject, .. } => match subject {
            ObjectScope::Source | ObjectScope::Anaphoric | ObjectScope::Demonstrative => {
                settled_on_source()
            }
            ObjectScope::Target
            | ObjectScope::Recipient
            | ObjectScope::EventSource
            | ObjectScope::CostPaidObject
            | ObjectScope::OtherRevealedCard
            | ObjectScope::OwnedLinkedExileCard
            | ObjectScope::EventTarget
            | ObjectScope::AmassedArmy
            | ObjectScope::ChainRootTarget
            | ObjectScope::GrantingObject
            | ObjectScope::SpecificObject { .. }
            | ObjectScope::BatchSource => ConditionOutcome::Undetermined,
        },
        // CR 601.2h: how the spell was paid for is stamped on its object when
        // it is cast; CR 400.7 clears the stamps when the object changes zones.
        AbilityCondition::CastVariantPaidInstead { .. }
        | AbilityCondition::CastTimingPermission { .. }
        | AbilityCondition::ManaColorSpent { .. } => settled_on_source(),
        AbilityCondition::ConditionInstead { inner } => {
            known_condition_outcome(inner, state, ability)
        }
        AbilityCondition::Not { condition } => {
            known_condition_outcome(condition, state, ability).negate()
        }
        AbilityCondition::And { conditions } => ConditionOutcome::all(
            conditions
                .iter()
                .map(|condition| known_condition_outcome(condition, state, ability)),
        ),
        AbilityCondition::Or { conditions } => ConditionOutcome::any(
            conditions
                .iter()
                .map(|condition| known_condition_outcome(condition, state, ability)),
        ),
        // CR 608.2c: reads the live keywords of the first object target, which
        // can change before resolution.
        AbilityCondition::TargetHasKeywordInstead { .. }
        // The outcome of an earlier instruction in the same resolution.
        | AbilityCondition::EffectOutcome { .. }
        | AbilityCondition::WhenYouDo
        | AbilityCondition::PreviousEffectAmount { .. }
        | AbilityCondition::ZoneChangedThisWay { .. }
        | AbilityCondition::RevealedHasCardType { .. }
        | AbilityCondition::ObjectsShareQuality { .. }
        | AbilityCondition::TargetSharesNameWithOtherExiledThisWay { .. }
        | AbilityCondition::DiscardedCardMatchesFilter { .. }
        | AbilityCondition::CoinFlipOutcome { .. }
        | AbilityCondition::EventOutcomeWon
        // The event that triggered the ability.
        | AbilityCondition::TriggerEventTargetDamagedBySourceThisTurn
        | AbilityCondition::TriggerEventTargetExploitedBySource
        | AbilityCondition::TriggeringSpellTargetsFilter { .. }
        | AbilityCondition::PostReplacementDamageSourceMatchesFilter { .. }
        | AbilityCondition::ZoneChangeObjectMatchesFilter { .. }
        | AbilityCondition::CostPaidObjectMatchesFilter { .. }
        // The game state at resolution.
        | AbilityCondition::CurrentPhaseIs { .. }
        | AbilityCondition::SourceEnteredThisTurn
        | AbilityCondition::QuantityCheck { .. }
        | AbilityCondition::HasMaxSpeed
        | AbilityCondition::IsMonarch
        | AbilityCondition::IsInitiative
        | AbilityCondition::HasCityBlessing
        | AbilityCondition::HasEnduringStory
        | AbilityCondition::ControlsCommander { .. }
        | AbilityCondition::IsRingBearer
        | AbilityCondition::CompletedDungeon { .. }
        | AbilityCondition::TargetMatchesFilter { .. }
        | AbilityCondition::HasObjectTarget
        | AbilityCondition::SourceMatchesFilter { .. }
        | AbilityCondition::ControllerControlsMatching { .. }
        | AbilityCondition::ControllerControlledMatchingAsCast { .. }
        | AbilityCondition::IsYourTurn
        | AbilityCondition::WasStartingPlayer { .. }
        | AbilityCondition::SpellCastWithVariantThisTurn { .. }
        | AbilityCondition::FirstCombatPhaseOfTurn
        | AbilityCondition::FirstEndStepOfTurn
        | AbilityCondition::SourceIsTapped
        | AbilityCondition::SourceAttachedToCreature
        | AbilityCondition::DayNightIsNeither
        | AbilityCondition::DayNightIs { .. }
        | AbilityCondition::AbilityUseCountThisTurn { .. }
        | AbilityCondition::SourceLacksKeyword { .. }
        | AbilityCondition::ScopedPlayerMatches { .. } => ConditionOutcome::Undetermined,
    }
}

/// CR 608.2c + CR 614.15: whether `sub` is an "instead" override of the
/// instruction before it. These four condition kinds are exactly the ones the
/// resolver treats that way.
fn is_instead_override(sub: &ResolvedAbility) -> bool {
    matches!(
        sub.condition,
        Some(
            AbilityCondition::AdditionalCostPaidInstead
                | AbilityCondition::CastVariantPaidInstead { .. }
                | AbilityCondition::TargetHasKeywordInstead { .. }
                | AbilityCondition::ConditionInstead { .. }
        )
    )
}

/// The outcome of `head`'s "instead" override sub, if it has one. Evaluated
/// against `head`, as the resolver's swap check does.
fn instead_override<'a>(
    state: &GameState,
    head: &'a ResolvedAbility,
) -> Option<(&'a ResolvedAbility, ConditionOutcome)> {
    let sub = head
        .sub_ability
        .as_deref()
        .filter(|sub| is_instead_override(sub))?;
    let condition = sub.condition.as_ref()?;
    Some((sub, known_condition_outcome(condition, state, head)))
}

/// Whether `link`'s effect can turn out other than as printed for reasons the
/// preview does not model: a choice made at resolution (CR 608.2d — "you
/// may", "unless", a random or per-player selection, a division, an optional
/// or resolution-timed target choice), a repeat, an unimplemented effect, or a
/// damage source rebound at resolution. The walk
/// reports nothing from such a link onward.
fn diverts(link: &ResolvedAbility) -> bool {
    link.optional
        || link.optional_for.is_some()
        || link.optional_player.is_some()
        || link.unless_pay.is_some()
        || link.player_scope.is_some()
        || link.repeat_for.is_some()
        || link.multi_target.is_some()
        || link.distribution.is_some()
        || link.distribute.is_some()
        || link.target_selection_mode == TargetSelectionMode::Random
        // CR 601.2c + CR 608.2d: "up to" targets may be left unchosen, and a
        // resolution-timed choice is made only while the effect resolves.
        || link.optional_targeting
        || link.target_choice_timing == TargetChoiceTiming::Resolution
        || matches!(
            link.effect,
            Effect::Unimplemented { .. }
                | Effect::DealDamage {
                    damage_source: Some(_),
                    ..
                }
        )
}

/// CR 608.2c: `child` as the resolver hands it off from `parent` — the
/// parent's targets when the child declares none of its own (CR 608.2b), and
/// the parent's casting-time context.
fn hand_off(parent: &ResolvedAbility, child: &ResolvedAbility) -> ResolvedAbility {
    let mut handed = child.clone();
    if should_propagate_parent_targets(parent, &handed) {
        handed.mirror_targets_from(parent);
    }
    inherit_parent_chain_context(&mut handed, parent, None);
    handed
}

/// CR 608.2c: the else branch a head with a false condition hands off to. The
/// resolver copies the head's context wholesale here rather than running the
/// full hand-off.
fn else_hand_off(head: &ResolvedAbility, else_branch: &ResolvedAbility) -> ResolvedAbility {
    let mut handed = else_branch.clone();
    if should_propagate_parent_targets(head, &handed) {
        handed.mirror_targets_from(head);
    }
    handed.context = head.context.clone();
    handed
}

/// CR 608.2c: the links of `ability`'s chain that will certainly be followed,
/// in resolution order, each with the targets and context it will resolve
/// with. An "instead" override that is settled to apply replaces the link it
/// overrides (CR 614.15), so the replaced effect is not reported.
///
/// The walk stops — reporting nothing further — at the first link whose path
/// depends on something not yet settled (see [`known_condition_outcome`]) or
/// whose effect can still be diverted at resolution. A link that is reported
/// is therefore a fact; a link that is not reported may or may not happen.
/// An unsettled "instead" override stops the walk before the head it may
/// replace; [`known_resolution_branches`] follows both of its outcomes.
pub fn known_resolution_links(
    state: &GameState,
    ability: &ResolvedAbility,
) -> Vec<ResolvedAbility> {
    let mut links = Vec::new();
    let mut next = Some(ability.clone());
    while let Some(head) = next.take() {
        match follow_head(state, head, &mut links) {
            Step::Next(following) => next = following.map(|head| *head),
            Step::Fork { .. } => break,
        }
    }
    links
}

/// CR 608.2c + CR 614.15: one list of certain links per way the chain can
/// still resolve. Where an "instead" override is not yet settled the chain
/// forks — one branch with the override swapped in, one with the head it
/// would replace — and each branch is walked on exactly as
/// [`known_resolution_links`] walks the whole chain. With nothing to fork on
/// there is a single branch, equal to [`known_resolution_links`].
///
/// A fact about the resolution holds only if it holds in every branch. The
/// result is never empty.
pub fn known_resolution_branches(
    state: &GameState,
    ability: &ResolvedAbility,
) -> Vec<Vec<ResolvedAbility>> {
    let mut branches = Vec::new();
    extend_branches(state, Some(ability.clone()), Vec::new(), &mut branches);
    branches
}

fn extend_branches(
    state: &GameState,
    mut next: Option<ResolvedAbility>,
    mut links: Vec<ResolvedAbility>,
    branches: &mut Vec<Vec<ResolvedAbility>>,
) {
    while let Some(head) = next.take() {
        match follow_head(state, head, &mut links) {
            Step::Next(following) => next = following.map(|head| *head),
            Step::Fork { swapped, kept } => {
                let mut swapped_links = links.clone();
                let after_swap = run_head(state, *swapped, &mut swapped_links);
                extend_branches(state, after_swap, swapped_links, branches);
                next = run_head(state, *kept, &mut links);
            }
        }
    }
    branches.push(links);
}

/// What follows one head of the walk.
enum Step {
    /// The chain continues determinately with this head, or ends.
    Next(Option<Box<ResolvedAbility>>),
    /// An unsettled "instead" override: the head resolves either as
    /// `swapped` (the override applied) or as `kept` (it did not).
    Fork {
        swapped: Box<ResolvedAbility>,
        kept: Box<ResolvedAbility>,
    },
}

/// Resolves one head the way `resolve_chain_body` does — the "instead" swap
/// before the head's own condition.
fn follow_head(state: &GameState, head: ResolvedAbility, links: &mut Vec<ResolvedAbility>) -> Step {
    // CR 614.15: an override settled to apply replaces the head before the
    // head's own condition is considered; the swapped head carries no
    // condition, and the override's own sub-ability continues the chain.
    match instead_override(state, &head) {
        Some((sub, ConditionOutcome::Holds)) => {
            let swapped = apply_instead_swap(&head, sub);
            Step::Next(run_head(state, swapped, links).map(Box::new))
        }
        Some((sub, ConditionOutcome::Undetermined)) => Step::Fork {
            swapped: Box::new(apply_instead_swap(&head, sub)),
            kept: Box::new(head),
        },
        Some((_, ConditionOutcome::Fails)) | None => {
            Step::Next(run_head(state, head, links).map(Box::new))
        }
    }
}

/// A head whose override, if any, is decided: the shape guard, then its own
/// condition. Pushes the head when it runs and returns the next head.
fn run_head(
    state: &GameState,
    head: ResolvedAbility,
    links: &mut Vec<ResolvedAbility>,
) -> Option<ResolvedAbility> {
    if diverts(&head) {
        return None;
    }
    match own_condition(state, &head) {
        ConditionOutcome::Holds => {
            let next = head
                .sub_ability
                .as_deref()
                .and_then(|child| descend(state, &head, child));
            links.push(head);
            next
        }
        // CR 608.2c: a false head hands off to its else chain only — its
        // sub-ability, including an override's not-swapped continuation,
        // belongs to the skipped instruction.
        ConditionOutcome::Fails => head
            .else_ability
            .as_deref()
            .map(|else_branch| else_hand_off(&head, else_branch)),
        ConditionOutcome::Undetermined => None,
    }
}

/// The head's own condition; an unconditional head always runs.
fn own_condition(state: &GameState, head: &ResolvedAbility) -> ConditionOutcome {
    head.condition
        .as_ref()
        .map_or(ConditionOutcome::Holds, |condition| {
            known_condition_outcome(condition, state, head)
        })
}

/// CR 608.2c: the sub-ability after `parent` ran. An "instead" override that
/// reaches this point was not swapped in, so only its continuation follows.
/// Any other sub is gated on its condition evaluated in the parent's frame,
/// as the resolver does; a false gate hands off to the sub's else branch.
fn descend(
    state: &GameState,
    parent: &ResolvedAbility,
    child: &ResolvedAbility,
) -> Option<ResolvedAbility> {
    if is_instead_override(child) {
        return not_swapped_continuation(parent, child);
    }
    let gate = child
        .condition
        .as_ref()
        .map_or(ConditionOutcome::Holds, |condition| {
            known_condition_outcome(condition, state, parent)
        });
    match gate {
        ConditionOutcome::Holds => Some(hand_off(parent, child)),
        ConditionOutcome::Fails => child
            .else_ability
            .as_deref()
            .map(|else_branch| hand_off(parent, else_branch)),
        ConditionOutcome::Undetermined => None,
    }
}

/// CR 608.2c + CR 614.15: what follows a head whose "instead" override did not
/// apply. The override's else branch is the base chain; without one, a later
/// independent sentence (a `SequentialSibling` tail) still runs, but a
/// continuation of the replaced clause does not, and an unimplemented tail is
/// never assumed.
fn not_swapped_continuation(
    head: &ResolvedAbility,
    override_sub: &ResolvedAbility,
) -> Option<ResolvedAbility> {
    if let Some(base_chain) = override_sub.else_ability.as_deref() {
        return Some(hand_off(head, base_chain));
    }
    override_sub
        .sub_ability
        .as_deref()
        .filter(|tail| {
            tail.sub_link == SubAbilityLink::SequentialSibling
                && !matches!(tail.effect, Effect::Unimplemented { .. })
        })
        .map(|tail| hand_off(head, tail))
}
