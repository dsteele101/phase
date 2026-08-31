//! CR 120.1 + CR 608.2b: a "<target creature> deals damage equal to its power
//! to <recipient>" clause whose SUBJECT is an illegal target at resolution must
//! deal no damage.
//!
//! Reported against Flesh // Blood's Blood half. The subject is declared by a
//! parent node of the ability chain and the damage clause reads it positionally
//! out of `targets[0]`, so once CR 608.2b pruned the dead subject away the
//! RECIPIENT slid into slot 0 and dealt its own power to itself: casting Blood
//! at a 1/1 you control and an opposing 3/1, then killing the 1/1 in response,
//! killed the 3/1.
//!
//! CR 608.2b: "If part of the effect requires information about an illegal
//! target, it fails to determine any such information. Any part of the effect
//! that requires that information won't happen." The clause needs the subject's
//! identity (CR 120.1: an object that deals damage is the source of that
//! damage) and its power, so it does nothing — while the rest of the chain
//! still resolves.
//!
//! These exercise the CHAIN SHAPE, not one card: `TargetOnly` and `Pump`
//! parents, `DealDamage` and `DamageAll` children, and a child carrying a
//! trailing clause of its own.

use engine::game::game_object::PhaseOutCause;
use engine::game::phasing::phase_out_object;
use engine::game::scenario::{GameScenario, P0, P1};
use engine::game::zone_pipeline::{move_object_for_test, ZoneMoveRequest};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaType, ManaUnit};
use engine::types::phase::Phase;
use engine::types::zones::Zone;

/// Verbatim text of BOTH Flesh // Blood's Blood half (the reported card) and
/// Soul's Fire. Named for Soul's Fire in these scenarios because "Blood" alone
/// is the Blood artifact token on Scryfall. Rabid Bite, Bite Down and Master's
/// Rebuke differ only in the recipient filter.
const SOULS_FIRE_ORACLE: &str =
    "Target creature you control deals damage equal to its power to any target.";

/// Ambuscade: a `Pump` parent supplies the subject and the amount is the
/// anaphoric "its power" rather than a `Target`-scoped read.
const AMBUSCADE_ORACLE: &str = "Target creature you control gets +1/+0 until end of turn. It deals damage equal to its power to target creature an opponent controls.";

/// Chandra's Ignition: the child is a `DamageAll` batch with no target slot of
/// its own.
const IGNITION_ORACLE: &str = "Target creature you control deals damage equal to its power to each other creature and each opponent.";

/// Shape probe (not a printed card): a trailing clause hanging off the damage
/// node. A non-interactive tail keeps the assertion about chain continuation
/// rather than about driving a resolution-time prompt.
const TRAILING_CLAUSE_ORACLE: &str = "Target creature you control deals damage equal to its power to target creature. You gain 3 life.";

fn floating_mana(n: usize, ty: ManaType) -> Vec<ManaUnit> {
    (0..n)
        .map(|_| ManaUnit::new(ty, ObjectId(0), false, vec![]))
        .collect()
}

fn red_mana(n: usize) -> Vec<ManaUnit> {
    floating_mana(n, ManaType::Red)
}

/// Kill `victim` outright, as a removal spell resolving in the response window
/// would (CR 704.5f state-based death lands it in the graveyard the same way).
fn kill(runner: &mut engine::game::scenario::GameRunner, victim: ObjectId, source: ObjectId) {
    let mut events = Vec::new();
    move_object_for_test(
        runner.state_mut(),
        ZoneMoveRequest::effect(victim, Zone::Graveyard, source),
        &mut events,
    );
}

/// The reported repro: the subject dies in response, and the "any target"
/// recipient must be untouched. The spell still RESOLVES — the recipient is
/// still a legal target, so CR 608.2b does not fizzle it.
#[test]
fn souls_fire_deals_no_damage_when_its_subject_dies_in_response() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Soul's Fire", false, SOULS_FIRE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Goblin", 1, 1).id();
    let recipient = scenario.add_creature(P1, "Bear", 3, 1).id();
    scenario.with_mana_pool(P0, red_mana(3));

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "recipient must take no damage once the subject is an illegal target"
    );
    assert!(
        runner.state().battlefield.contains(&recipient),
        "recipient must survive — it was never the damage source"
    );
    assert!(
        runner.state().stack.is_empty(),
        "spell must finish resolving, not stay on the stack"
    );
}

/// Same shape with a PLAYER recipient. The bug's other face: with no object in
/// `targets[0]` the source fell back to the spell itself, which would have
/// dealt damage attributed to a spell that had no subject.
#[test]
fn souls_fire_deals_no_damage_to_player_when_its_subject_dies_in_response() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Soul's Fire", false, SOULS_FIRE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Goblin", 4, 4).id();
    scenario.with_mana_pool(P0, red_mana(3));

    let mut runner = scenario.build();
    let life_before = runner.state().players[1].life;
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject])
            .target_player(P1)
            .commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().players[1].life,
        life_before,
        "no object deals the damage, so the player loses no life"
    );
}

/// Control: nothing became illegal, so the happy path must be untouched. This
/// is the regression guard for the ~90 cards in this class.
#[test]
fn souls_fire_deals_its_subjects_power_when_everything_stays_legal() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Soul's Fire", false, SOULS_FIRE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Ogre", 4, 4).id();
    let recipient = scenario.add_creature(P1, "Wall", 0, 7).id();
    scenario.with_mana_pool(P0, red_mana(3));

    let mut runner = scenario.build();
    runner
        .cast(spell)
        .target_objects(&[subject, recipient])
        .commit()
        .resolve();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        4,
        "recipient takes damage equal to the SUBJECT's power, not its own"
    );
    assert_eq!(
        runner.state().objects[&subject].damage_marked,
        0,
        "the subject deals damage, it does not receive it"
    );
}

/// The mirror case: the recipient becomes illegal while the subject survives.
/// Nothing is damaged, and in particular the subject must not be re-read as its
/// own recipient.
#[test]
fn souls_fire_deals_no_damage_when_its_recipient_dies_in_response() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Soul's Fire", false, SOULS_FIRE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Ogre", 4, 4).id();
    let recipient = scenario.add_creature(P1, "Bear", 2, 2).id();
    scenario.with_mana_pool(P0, red_mana(3));

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    kill(&mut runner, recipient, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&subject].damage_marked,
        0,
        "subject must not be promoted into its own recipient slot"
    );
    assert!(
        runner.state().battlefield.contains(&subject),
        "subject must survive"
    );
}

/// A phased-out subject is an illegal target too (CR 702.26b: treated as though
/// it does not exist), and unlike death it leaves the object on the battlefield
/// — so a zone check alone would miss it.
#[test]
fn souls_fire_deals_no_damage_when_its_subject_phases_out_in_response() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Soul's Fire", false, SOULS_FIRE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Ogre", 4, 4).id();
    let recipient = scenario.add_creature(P1, "Bear", 3, 3).id();
    scenario.with_mana_pool(P0, red_mana(3));

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    let mut events = Vec::new();
    phase_out_object(
        runner.state_mut(),
        subject,
        PhaseOutCause::Directly,
        &mut events,
    );
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "a phased-out subject deals no damage (CR 702.26b)"
    );
}

/// `Pump` parent + anaphoric "its power" (Ambuscade, Clear Shot, Wolf Strike).
/// The subject slot is declared by a boost clause rather than a bare
/// `TargetOnly`, so this proves the gate keys off the DECLARED slot, not off one
/// specific parent effect.
#[test]
fn pump_subject_chain_deals_no_damage_when_its_subject_dies_in_response() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Ambuscade", false, AMBUSCADE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Ogre", 3, 3).id();
    let recipient = scenario.add_creature(P1, "Bear", 4, 4).id();
    scenario.with_mana_pool(
        P0,
        floating_mana(1, ManaType::Green)
            .into_iter()
            .chain(floating_mana(1, ManaType::Colorless))
            .collect(),
    );

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "a boosted subject that died deals no damage"
    );
    assert!(
        runner.state().battlefield.contains(&recipient),
        "recipient must survive"
    );
}

/// `DamageAll` child (Chandra's Ignition, Alpha Brawl, Waltz of Rage). The
/// child has no target slot of its own, and the resolved subject also drives
/// the `Another` exclusion on the recipient SET — so a lost subject must
/// cancel the whole sweep, not sweep with the spell as its source.
#[test]
fn damage_all_chain_wipes_nothing_when_its_subject_dies_in_response() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Chandra's Ignition", false, IGNITION_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Dragon", 5, 5).id();
    let bystander = scenario.add_creature(P1, "Bear", 2, 2).id();
    scenario.with_mana_pool(P0, red_mana(5));

    let mut runner = scenario.build();
    let life_before = runner.state().players[1].life;
    {
        let _commit = runner.cast(spell).target_objects(&[subject]).commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&bystander].damage_marked,
        0,
        "no board wipe once the subject is an illegal target"
    );
    assert!(
        runner.state().battlefield.contains(&bystander),
        "bystander must survive"
    );
    assert_eq!(
        runner.state().players[1].life,
        life_before,
        "opponents lose no life either"
    );
}

/// The damage clause is nullified, but a clause AFTER it is a separate
/// instruction and still happens (CR 608.2b nullifies only the parts that need
/// the illegal target's information). Regression guard for the ~18 cards in
/// this class that carry a trailing clause — Contest of Claws' Discover,
/// Burn Together's Sacrifice, Hulk's Thunderclap's Destroy.
#[test]
fn trailing_clause_still_resolves_when_the_damage_subject_dies() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Trailing Clause Probe", false, TRAILING_CLAUSE_ORACLE)
        .id();
    let subject = scenario.add_creature(P0, "Ogre", 4, 4).id();
    let recipient = scenario.add_creature(P1, "Bear", 3, 3).id();
    scenario.with_mana_pool(P0, red_mana(3));

    let mut runner = scenario.build();
    let life_before = runner.state().players[0].life;
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "the damage clause is the only part silenced"
    );
    assert_eq!(
        runner.state().players[0].life,
        life_before + 3,
        "a clause after the damage is a separate instruction and still happens"
    );
}

/// Throw from the Saddle — the damage clause is a `SequentialSibling` TAIL of a
/// `ConditionInstead` override, two levels down:
///
///   Pump  ->  PutCounter { ConditionInstead: subject is a Mount }
///                 -> DealDamage { Power{Anaphoric}, damage_source: Target }
///
/// The tail is delivered by the not-swap arm's OWN descent, separate from the
/// ordinary chain path, so it needs the same subject classification.
///
/// MEASURED, not assumed: an illegal subject reaches this route from BOTH
/// subtypes. The override's condition ("if it's a Mount") reads the subject
/// itself, so once the subject is an illegal target the condition cannot
/// evaluate true — the swap never fires and a Mount subject funnels into the
/// same not-swap arm. Probed at the descent: both cases print
/// `parent_targets=[]` at the not-swap tail runner and never reach the ordinary
/// chain classifier. The swap descent is therefore unreachable with an illegal
/// subject for this card; it is covered anyway because both descents now share
/// one classifier.
const THROW_FROM_THE_SADDLE_ORACLE: &str =
    "Target creature you control gets +1/+1 until end of turn. \
Put a +1/+1 counter on it instead if it's a Mount. \
Then it deals damage equal to its power to target creature you don't control.";

/// Non-Mount subject: the override does not fire, so the tail is delivered by
/// the not-swap arm's own descent. This is the reported-shape regression.
#[test]
fn condition_instead_tail_deals_no_damage_when_its_subject_dies_in_response() {
    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Throw from the Saddle",
            false,
            THROW_FROM_THE_SADDLE_ORACLE,
        )
        .id();
    let subject = scenario.add_creature(P0, "Plains Rider", 4, 4).id();
    let recipient = scenario.add_creature(P1, "Opposing Bear", 9, 9).id();

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "the instead-tail damage clause must not turn its recipient into its own source"
    );
    assert!(
        runner.state().battlefield.contains(&recipient),
        "recipient must survive"
    );
}

/// Mount subject: the override would fire on a legal subject, but a dead
/// subject cannot satisfy "if it's a Mount", so this funnels into the not-swap
/// arm too. Kept as a distinct case so a future change that makes the swap
/// reachable with an illegal subject is still covered here.
#[test]
fn condition_instead_mount_subject_deals_no_damage_when_its_subject_dies_in_response() {
    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Throw from the Saddle",
            false,
            THROW_FROM_THE_SADDLE_ORACLE,
        )
        .id();
    let subject = scenario
        .add_creature(P0, "Mounted Knight", 4, 4)
        .with_subtypes(vec!["Mount"])
        .id();
    let recipient = scenario.add_creature(P1, "Opposing Bear", 9, 9).id();

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    kill(&mut runner, subject, spell);
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "the Mount-subject instead-tail must not turn its recipient into its own source"
    );
    assert!(
        runner.state().battlefield.contains(&recipient),
        "recipient must survive"
    );
}

/// Phase-out variant of the not-swap route: the subject stays on the
/// battlefield, so a zone check alone would miss it (CR 702.26b).
#[test]
fn condition_instead_tail_deals_no_damage_when_its_subject_phases_out_in_response() {
    let mut scenario = GameScenario::new_n_player(2, 42);
    scenario.at_phase(Phase::PreCombatMain);

    let spell = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Throw from the Saddle",
            false,
            THROW_FROM_THE_SADDLE_ORACLE,
        )
        .id();
    let subject = scenario.add_creature(P0, "Plains Rider", 4, 4).id();
    let recipient = scenario.add_creature(P1, "Opposing Bear", 9, 9).id();

    let mut runner = scenario.build();
    {
        let _commit = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .commit();
    }

    let mut events = Vec::new();
    phase_out_object(
        runner.state_mut(),
        subject,
        PhaseOutCause::Directly,
        &mut events,
    );
    runner.advance_until_stack_empty();

    assert_eq!(
        runner.state().objects[&recipient].damage_marked,
        0,
        "a phased-out subject deals no damage through the instead-tail route either"
    );
}

/// Control for both routes: nothing became illegal, so the tail must still deal
/// the subject's live power. Guards against fixing the bug by silencing the tail.
#[test]
fn condition_instead_tail_still_deals_damage_when_everything_stays_legal() {
    for (subtypes, expected) in [(Vec::new(), 5u32), (vec!["Mount"], 5u32)] {
        let mut scenario = GameScenario::new_n_player(2, 42);
        scenario.at_phase(Phase::PreCombatMain);

        let spell = scenario
            .add_spell_to_hand_from_oracle(
                P0,
                "Throw from the Saddle",
                false,
                THROW_FROM_THE_SADDLE_ORACLE,
            )
            .id();
        let subject = {
            let mut builder = scenario.add_creature(P0, "Rider", 4, 4);
            if subtypes.is_empty() {
                builder.id()
            } else {
                builder.with_subtypes(subtypes.clone()).id()
            }
        };
        let recipient = scenario.add_creature(P1, "Opposing Bear", 9, 9).id();

        let mut runner = scenario.build();
        let outcome = runner
            .cast(spell)
            .target_objects(&[subject, recipient])
            .resolve();

        assert_eq!(
            outcome.state().objects[&recipient].damage_marked,
            expected,
            "legal subject (subtypes {subtypes:?}) must still deal its live power through the tail"
        );
        assert_eq!(
            outcome.state().objects[&subject].damage_marked,
            0,
            "the subject is the source, not a recipient"
        );
    }
}
