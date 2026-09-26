//! Master of the Wild Hunt — Phase 1 runtime rows (clause 2). Clause 3's
//! division is not implemented in this phase; these rows assert only the
//! target prompt, the tapping, and each tapped Wolf's damage.
//!
//! CR 602.2b + CR 601.2c: the activation announces clause 2's target.
//! CR 701.26a + CR 608.2c: only permanents the instruction actually taps are
//!   "tapped this way".
//! CR 120.1: each such Wolf is the source of damage equal to its own power.
//! CR 608.2b: an ability whose only target is illegal does not resolve.

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::statics::StaticMode;
use engine::types::zones::Zone;

use crate::rules::{drive_with_response, PriorityResponse};

/// Verbatim Oracle text (Scryfall / MTGJSON). Creature — Human Shaman, 3/3.
const MASTER: &str = "At the beginning of your upkeep, create a 2/2 green Wolf creature token.\n\
{T}: Tap all untapped Wolf creatures you control. Each Wolf tapped this way deals damage equal to \
its power to target creature. That creature deals damage equal to its power divided as its \
controller chooses among any number of those Wolves.";

fn wolf(s: &mut GameScenario, owner: PlayerId, name: &str, power: i32, toughness: i32) -> ObjectId {
    s.add_creature(owner, name, power, toughness)
        .with_subtypes(vec!["Wolf"])
        .id()
}

fn master(s: &mut GameScenario) -> ObjectId {
    s.add_creature_from_oracle(P0, "Master of the Wild Hunt", 3, 3, MASTER)
        .with_subtypes(vec!["Human", "Shaman"])
        .id()
}

/// Master's only activated ability ({T}: …) is ability index 0; the upkeep
/// line is a trigger, not an activated ability.
fn activate_master(master: ObjectId) -> GameAction {
    GameAction::ActivateAbility {
        source_id: master,
        ability_index: 0,
    }
}

fn tapped(runner: &GameRunner, id: ObjectId) -> bool {
    runner.state().objects[&id].tapped
}

fn damage(runner: &GameRunner, id: ObjectId) -> u32 {
    runner.state().objects[&id].damage_marked
}

/// Master, P0 Wolves 1/1 and 4/4, a pre-tapped P0 Wolf 2/2, and P1's 6/6.
struct Board {
    runner: GameRunner,
    master: ObjectId,
    w1: ObjectId,
    w4: ObjectId,
    pre_tapped: ObjectId,
    giant: ObjectId,
}

fn board() -> Board {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let master = master(&mut s);
    let w1 = wolf(&mut s, P0, "Wolf One", 1, 1);
    let w4 = wolf(&mut s, P0, "Wolf Four", 4, 4);
    let pre_tapped = wolf(&mut s, P0, "Tired Wolf", 2, 2);
    let giant = s.add_creature(P1, "Target Giant", 6, 6).id();
    let mut runner = s.build();
    runner
        .state_mut()
        .objects
        .get_mut(&pre_tapped)
        .unwrap()
        .tapped = true;
    Board {
        runner,
        master,
        w1,
        w4,
        pre_tapped,
        giant,
    }
}

/// CR 602.2b + CR 601.2c: activating Master announces clause 2's "target
/// creature" even though the root tap declares no target (the reported bug).
#[test]
fn activation_prompts_for_target_creature() {
    let Board {
        mut runner,
        master,
        giant,
        ..
    } = board();
    runner
        .act(activate_master(master))
        .expect("activate Master of the Wild Hunt");

    match &runner.state().waiting_for {
        WaitingFor::TargetSelection { target_slots, .. } => {
            assert_eq!(target_slots.len(), 1, "exactly clause 2's target slot");
            assert!(
                target_slots[0]
                    .legal_targets
                    .contains(&TargetRef::Object(giant)),
                "the opponent's creature is a legal target"
            );
        }
        other => panic!("activation must prompt for a target creature, got {other:?}"),
    }

    runner
        .act(GameAction::ChooseTarget {
            target: Some(TargetRef::Object(giant)),
        })
        .expect("choose the target creature");
    assert_eq!(
        runner.state().stack.len(),
        1,
        "reach guard: the ability is on the stack after its target is chosen"
    );
}

/// CR 701.26a + CR 120.1: each Wolf the instruction taps deals its own power;
/// the pre-tapped Wolf and Master itself are not sources.
#[test]
fn each_wolf_tapped_this_way_deals_its_power_to_target() {
    let Board {
        mut runner,
        master,
        w1,
        w4,
        pre_tapped,
        giant,
    } = board();
    runner.activate(master, 0).target_object(giant).resolve();

    assert!(tapped(&runner, master), "Master tapped for its cost");
    assert!(tapped(&runner, w1), "untapped Wolf 1/1 becomes tapped");
    assert!(tapped(&runner, w4), "untapped Wolf 4/4 becomes tapped");
    assert!(tapped(&runner, pre_tapped), "pre-tapped Wolf stays tapped");
    assert_eq!(
        damage(&runner, giant),
        5,
        "only the Wolves tapped this way deal their own power (1 + 4); the pre-tapped \
         Wolf (7) and Master (3) are not sources"
    );
    for id in [master, w1, w4, pre_tapped] {
        assert_eq!(
            damage(&runner, id),
            0,
            "clause 3 is not implemented in this phase"
        );
    }
    assert!(runner.state().stack.is_empty());
}

/// CR 701.26a: a Wolf that can't become tapped and an opponent's Wolf are not
/// tapped by the instruction, so neither is a source.
#[test]
fn wolf_that_cannot_tap_and_opponents_wolf_deal_no_damage() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let master = master(&mut s);
    let small = wolf(&mut s, P0, "Small Wolf", 2, 2);
    let held = s
        .add_creature(P0, "Held Wolf", 8, 8)
        .with_subtypes(vec!["Wolf"])
        .with_static(StaticMode::CantTap)
        .id();
    let theirs = wolf(&mut s, P1, "Their Wolf", 4, 4);
    let giant = s.add_creature(P1, "Target Giant", 6, 6).id();
    let mut runner = s.build();

    runner.activate(master, 0).target_object(giant).resolve();

    assert!(tapped(&runner, small), "P0's 2/2 Wolf is tapped this way");
    assert!(
        !tapped(&runner, held),
        "reach guard: the can't-tap restriction kept the 8/8 Wolf upright"
    );
    assert!(
        !tapped(&runner, theirs),
        "an opponent's Wolf is not a Wolf you control"
    );
    assert_eq!(
        damage(&runner, giant),
        2,
        "only the 2/2 Wolf tapped this way deals damage (not 10, 6, or 14)"
    );
}

/// No Wolves you control: Master taps, nothing is tapped this way, and the
/// target — itself an opponent's Wolf — takes no damage.
#[test]
fn no_wolves_taps_master_and_deals_no_damage() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let master = master(&mut s);
    let target_wolf = wolf(&mut s, P1, "Target Wolf", 6, 6);
    let mut runner = s.build();

    runner
        .activate(master, 0)
        .target_object(target_wolf)
        .resolve();

    assert!(tapped(&runner, master), "Master tapped for its cost");
    assert!(
        !tapped(&runner, target_wolf),
        "the opponent's Wolf is not tapped"
    );
    assert_eq!(
        damage(&runner, target_wolf),
        0,
        "a declared target is never a member of the mass tap's set"
    );
    assert!(runner.state().stack.is_empty());
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::Priority { .. }
    ));
}

/// CR 608.2c: the "tapped this way" set belongs to one resolution. A second
/// activation that taps nothing must not reuse the first activation's set.
#[test]
fn second_activation_does_not_reuse_prior_tapped_set() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let master = master(&mut s);
    let w4 = wolf(&mut s, P0, "Wolf Four", 4, 4);
    let giant = s.add_creature(P1, "Target Giant", 6, 6).id();
    let mut runner = s.build();

    runner.activate(master, 0).target_object(giant).resolve();
    assert_eq!(
        damage(&runner, giant),
        4,
        "first activation: the 4/4 deals 4"
    );
    assert!(tapped(&runner, w4));

    runner.state_mut().objects.get_mut(&master).unwrap().tapped = false;
    runner.activate(master, 0).target_object(giant).resolve();

    assert!(
        tapped(&runner, master),
        "reach guard: the second activation paid its cost"
    );
    assert!(
        runner.state().stack.is_empty(),
        "the second activation resolved"
    );
    assert_eq!(
        damage(&runner, giant),
        4,
        "the already-tapped Wolf is not tapped this way by the second activation"
    );
}

/// CR 608.2b + ruling (2018-03-16): if the target is illegal on resolution,
/// the ability doesn't resolve — no Wolf becomes tapped and no damage is dealt.
#[test]
fn illegal_target_prevents_wolf_tap_and_damage() {
    let mut s = GameScenario::new();
    s.at_phase(Phase::PreCombatMain);
    let master = master(&mut s);
    let w1 = wolf(&mut s, P0, "Wolf One", 1, 1);
    let w4 = wolf(&mut s, P0, "Wolf Four", 4, 4);
    let giant = s.add_creature(P1, "Target Giant", 6, 6).id();
    let murder = s
        .add_spell_to_hand_from_oracle(P1, "Murder", true, "Destroy target creature.")
        .id();
    let mut runner = s.build();

    drive_with_response(
        &mut runner,
        activate_master(master),
        &[giant],
        Some(PriorityResponse {
            player: P1,
            instant: murder,
            target: giant,
        }),
    );

    assert_eq!(
        runner.state().objects[&giant].zone,
        Zone::Graveyard,
        "reach guard: the response destroyed the target"
    );
    assert!(tapped(&runner, master), "Master's {{T}} cost was paid");
    for w in [w1, w4] {
        assert!(
            !tapped(&runner, w),
            "no Wolf is tapped when the ability fizzles"
        );
        assert_eq!(damage(&runner, w), 0);
    }
    assert!(runner.state().stack.is_empty());
}
