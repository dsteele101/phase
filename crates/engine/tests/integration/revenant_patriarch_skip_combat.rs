//! Revenant Patriarch: "When this creature enters, if {W} was spent to cast
//! it, target player skips their next combat phase." and the rest of the
//! whole-combat-phase skips, driven through `GameAction::PassPriority` so they
//! exercise the path players use (CR 500.11, CR 614.10, CR 614.10a).

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{Duration, TargetRef};
use engine::types::actions::GameAction;
use engine::types::game_state::{CastPaymentMode, ExtraPhase, PendingCombatPhaseSkip, WaitingFor};
use engine::types::identifiers::{ExtraPhaseId, ObjectId};
use engine::types::mana::{ManaCost, ManaCostShard, ManaType, ManaUnit};
use engine::types::phase::{Phase, PhaseGroup, TurnSegment};
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

/// Verbatim Oracle text (Scryfall).
const PATRIARCH_ORACLE: &str = "When this creature enters, if {W} was spent to cast it, target player skips their next combat phase.\nThis creature can't block.";
const MOMENT_OF_SILENCE_ORACLE: &str = "Target player skips their next combat phase this turn.";
const VEDALKEN_ORRERY_ORACLE: &str = "You may cast spells as though they had flash.";
const FALSE_PEACE_ORACLE: &str = "Target player skips all combat phases of their next turn.";

const BEAR_POWER: i32 = 2;
const P0_STARTING_LIFE: i32 = 20;
const P1_STARTING_LIFE: i32 = 20;

/// P0 holds the skip effects and P1 has what a combat phase would use: an
/// attacker and a beginning of combat trigger.
struct Table {
    runner: GameRunner,
    patriarch: ObjectId,
    moment: ObjectId,
    false_peace: ObjectId,
    bear: ObjectId,
}

fn table() -> Table {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let library = ["Island"; 12];
    scenario.with_library_top(P0, &library);
    scenario.with_library_top(P1, &library);
    let patriarch = {
        let mut builder = scenario.add_creature_to_hand(P0, "Revenant Patriarch", 4, 3);
        builder.from_oracle_text(PATRIARCH_ORACLE);
        builder.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Black],
            generic: 4,
        });
        builder.id()
    };
    let moment = {
        let mut builder = scenario.add_spell_to_hand_from_oracle(
            P0,
            "Moment of Silence",
            true,
            MOMENT_OF_SILENCE_ORACLE,
        );
        builder.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::White],
            generic: 0,
        });
        builder.id()
    };
    let false_peace = {
        let mut builder =
            scenario.add_spell_to_hand_from_oracle(P0, "False Peace", false, FALSE_PEACE_ORACLE);
        builder.with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red],
            generic: 0,
        });
        builder.id()
    };
    scenario.add_artifact_from_oracle(P0, "Vedalken Orrery", VEDALKEN_ORRERY_ORACLE);
    let bear = scenario.add_vanilla(P1, BEAR_POWER, 2);
    scenario.add_enchantment_from_oracle(
        P1,
        "Combat Watcher",
        "At the beginning of combat on your turn, you gain 1 life.",
    );
    Table {
        runner: scenario.build(),
        patriarch,
        moment,
        false_peace,
        bear,
    }
}

fn add_mana(runner: &mut GameRunner, player: PlayerId, mana_type: ManaType, amount: u32) {
    let pool = &mut runner
        .state_mut()
        .players
        .iter_mut()
        .find(|p| p.id == player)
        .unwrap()
        .mana_pool;
    for _ in 0..amount {
        pool.add(ManaUnit::new(mana_type, ObjectId(0), false, vec![]));
    }
}

/// Cast `spell`, answer every target prompt with `target`, and resolve
/// everything back to an empty stack. Returns whether a target prompt or the
/// spell's own ability was answered (reach guard).
fn cast_and_resolve(runner: &mut GameRunner, spell: ObjectId, target: PlayerId) -> bool {
    let card_id = runner.state().objects[&spell].card_id;
    let phase = runner.state().phase;
    let active = runner.state().active_player;
    runner
        .act(GameAction::CastSpell {
            object_id: spell,
            card_id,
            targets: vec![],
            payment_mode: CastPaymentMode::Auto,
        })
        .expect("cast accepted");

    let mut target_chosen = false;
    for _ in 0..40 {
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::TriggerTargetSelection { .. }
            | WaitingFor::TargetSelection { .. }
            | WaitingFor::MultiTargetSelection { .. } => {
                target_chosen = true;
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Player(target)),
                    })
                    .or_else(|_| {
                        runner.act(GameAction::SelectTargets {
                            targets: vec![TargetRef::Player(target)],
                        })
                    })
                    .expect("target accepted");
            }
            WaitingFor::Priority { .. } => {
                runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected waiting state: {other:?}"),
        }
    }
    assert!(runner.state().stack.is_empty(), "everything resolved");
    assert_eq!(
        (runner.state().phase, runner.state().active_player),
        (phase, active),
        "resolving must not leave the step the effect was cast in"
    );
    target_chosen
}

/// Cast Patriarch paying `white` W and `black` B, targeting `target` with its
/// enters trigger.
fn cast_patriarch(table: &mut Table, white: u32, black: u32, target: PlayerId) -> bool {
    add_mana(&mut table.runner, P0, ManaType::White, white);
    add_mana(&mut table.runner, P0, ManaType::Black, black);
    cast_and_resolve(&mut table.runner, table.patriarch, target)
}

fn cast_moment(table: &mut Table, target: PlayerId) {
    add_mana(&mut table.runner, P0, ManaType::White, 1);
    assert!(cast_and_resolve(&mut table.runner, table.moment, target));
    assert_eq!(
        table.runner.state().objects[&table.moment].zone,
        Zone::Graveyard,
        "Moment of Silence resolved"
    );
}

/// Pass priority from wherever the game is until `active`'s turn reaches
/// `phase` with an empty stack, attacking with P1's bear whenever P1 is asked.
/// Returns how many times P1 was asked to declare attackers.
fn pass_to(table: &mut Table, active: PlayerId, phase: Phase) -> u32 {
    let mut attack_prompts = 0;
    for _ in 0..400 {
        let state = table.runner.state();
        let waiting = state.waiting_for.clone();
        if state.active_player == active
            && state.phase == phase
            && state.stack.is_empty()
            && matches!(waiting, WaitingFor::Priority { .. })
        {
            return attack_prompts;
        }
        match waiting {
            WaitingFor::DeclareAttackers { player, .. } => {
                let attacks = if player == P1 {
                    attack_prompts += 1;
                    vec![(table.bear, AttackTarget::Player(P0))]
                } else {
                    vec![]
                };
                table
                    .runner
                    .declare_attackers(&attacks)
                    .expect("declare attackers");
            }
            WaitingFor::DeclareBlockers { .. } => {
                table.runner.declare_blockers(&[]).expect("no blocks");
            }
            WaitingFor::Priority { .. } => {
                table.runner.act(GameAction::PassPriority).expect("pass");
            }
            other => panic!("unexpected waiting state: {other:?}"),
        }
    }
    panic!("never reached {phase:?} on {active:?}'s turn");
}

/// Play P1's whole combat from their precombat main phase. Returns the attack
/// prompts P1 saw there.
fn play_p1_combat(table: &mut Table) -> u32 {
    pass_to(table, P1, Phase::PreCombatMain);
    pass_to(table, P1, Phase::PostCombatMain)
}

fn begin_combats_this_turn(table: &Table) -> u32 {
    table
        .runner
        .state()
        .steps_started_this_turn
        .count(Phase::BeginCombat)
}

fn pending_skips(table: &Table, player: PlayerId) -> usize {
    table
        .runner
        .state()
        .pending_combat_phase_skips
        .iter()
        .filter(|skip| skip.player == player)
        .count()
}

/// Asserts P1's combat just ran in full, once.
fn assert_combat_ran(table: &Table, prompts: u32, combats: u32) {
    assert_eq!(prompts, combats, "P1 is asked to attack");
    assert_eq!(begin_combats_this_turn(table), combats);
}

/// Asserts the combat P1 was due just now did not exist: no trigger, no prompt,
/// no attack, and the postcombat main phase follows the precombat one.
fn assert_combat_skipped(table: &Table, prompts: u32, life: (i32, i32)) {
    let state = table.runner.state();
    assert_eq!(state.phase, Phase::PostCombatMain);
    assert_eq!(prompts, 0, "no DeclareAttackers prompt");
    assert_eq!(
        begin_combats_this_turn(table),
        0,
        "no beginning of combat step"
    );
    assert_eq!(
        (table.runner.life(P0), table.runner.life(P1)),
        life,
        "no beginning of combat trigger and no combat damage"
    );
    assert!(state.combat.is_none(), "no combat state was created");
    assert!(
        !state.objects[&table.bear].tapped,
        "the bear did not attack"
    );
}

#[test]
fn white_spent_makes_target_player_skip_next_combat() {
    let mut table = table();
    assert!(cast_patriarch(&mut table, 1, 4, P1), "trigger must target");
    assert_eq!(pending_skips(&table, P1), 1, "P1 must skip next combat");
    assert_eq!(pending_skips(&table, P0), 0);
    assert!(
        table.runner.state().steps_to_skip[P1.0 as usize].is_empty(),
        "a whole phase is not armed as step counters"
    );

    // P1's next combat is skipped straight to postcombat main.
    let prompts = play_p1_combat(&mut table);
    assert_combat_skipped(&table, prompts, (P0_STARTING_LIFE, P1_STARTING_LIFE));
    assert_eq!(pending_skips(&table, P1), 0, "skip fully consumed");

    // P1's following combat runs: prompt, trigger and damage.
    let prompts = {
        pass_to(&mut table, P0, Phase::End);
        play_p1_combat(&mut table)
    };
    assert_combat_ran(&table, prompts, 1);
    assert_eq!(table.runner.life(P0), P0_STARTING_LIFE - BEAR_POWER);
    assert_eq!(table.runner.life(P1), P1_STARTING_LIFE + 1);
}

#[test]
fn no_white_spent_does_not_skip_combat() {
    let mut table = table();
    cast_patriarch(&mut table, 0, 5, P1);
    assert_eq!(pending_skips(&table, P1), 0);
    assert_eq!(pending_skips(&table, P0), 0);

    let prompts = play_p1_combat(&mut table);
    assert_combat_ran(&table, prompts, 1);
    assert_eq!(table.runner.life(P0), P0_STARTING_LIFE - BEAR_POWER);
}

/// The caster's own combat this turn is another player's: it neither uses up
/// nor is affected by P1's skip.
#[test]
fn another_players_combat_does_not_use_up_the_skip() {
    let mut table = table();
    cast_patriarch(&mut table, 1, 4, P1);

    pass_to(&mut table, P0, Phase::PostCombatMain);
    assert_eq!(begin_combats_this_turn(&table), 1, "P0's combat runs");
    assert_eq!(pending_skips(&table, P1), 1);
}

/// CR 614.10: a skip created after a combat phase has started waits for the
/// next occurrence. Revenant Patriarch cast with Vedalken Orrery during P1's
/// beginning of combat leaves that combat untouched and takes P1's next one.
#[test]
fn skip_created_during_a_combat_leaves_that_combat_untouched() {
    let mut table = table();
    pass_to(&mut table, P1, Phase::BeginCombat);
    table.runner.act(GameAction::PassPriority).expect("pass");
    assert_eq!(table.runner.state().phase, Phase::BeginCombat);
    assert!(cast_patriarch(&mut table, 1, 4, P1));
    assert_eq!(pending_skips(&table, P1), 1);

    let prompts = pass_to(&mut table, P1, Phase::PostCombatMain);
    assert_combat_ran(&table, prompts, 1);
    assert_eq!(table.runner.life(P0), P0_STARTING_LIFE - BEAR_POWER);
    assert_eq!(pending_skips(&table, P1), 1, "still waiting");

    let life = (table.runner.life(P0), table.runner.life(P1));
    pass_to(&mut table, P0, Phase::End);
    let prompts = play_p1_combat(&mut table);
    assert_combat_skipped(&table, prompts, life);
    assert_eq!(pending_skips(&table, P1), 0);
}

/// CR 500.8 + CR 614.10: the skip lands on the next combat phase, an additional
/// combat included, and CR 614.10a it is satisfied there: Moment of Silence is
/// used up within the turn it was cast.
#[test]
fn skip_lands_on_an_additional_combat_phase_of_the_same_turn() {
    let mut table = table();
    pass_to(&mut table, P1, Phase::BeginCombat);
    table.runner.state_mut().extra_phases.push(ExtraPhase {
        anchor: Phase::EndCombat,
        segment: TurnSegment::Phase(PhaseGroup::Combat),
        attacker_restriction: None,
        attacker_restriction_source: None,
        id: ExtraPhaseId::default(),
    });
    table.runner.act(GameAction::PassPriority).expect("pass");
    cast_moment(&mut table, P1);
    assert_eq!(pending_skips(&table, P1), 1);

    let prompts = pass_to(&mut table, P1, Phase::PostCombatMain);
    assert_combat_ran(&table, prompts, 1);
    assert_eq!(pending_skips(&table, P1), 0, "used up by the added combat");
    assert!(table.runner.state().extra_phases.is_empty());
    assert_eq!(table.runner.life(P1), P1_STARTING_LIFE + 1, "one trigger");
}

/// CR 514.2: Moment of Silence cast on a player whose turn it isn't ends at
/// cleanup without touching that player's next turn.
#[test]
fn moment_of_silence_expires_at_cleanup_on_another_players_turn() {
    let mut table = table();
    cast_moment(&mut table, P1);
    assert_eq!(pending_skips(&table, P1), 1, "reach guard: the skip armed");

    pass_to(&mut table, P1, Phase::Upkeep);
    assert_eq!(pending_skips(&table, P1), 0, "ended with the turn");

    let prompts = play_p1_combat(&mut table);
    assert_combat_ran(&table, prompts, 1);
}

/// CR 514.2 + CR 614.10: cast during the target's only combat of the turn, the
/// skip finds no further combat and does not carry over.
#[test]
fn moment_of_silence_during_the_only_combat_expires_unused() {
    let mut table = table();
    pass_to(&mut table, P1, Phase::BeginCombat);
    table.runner.act(GameAction::PassPriority).expect("pass");
    cast_moment(&mut table, P1);
    assert_eq!(pending_skips(&table, P1), 1, "reach guard: the skip armed");

    let prompts = pass_to(&mut table, P1, Phase::PostCombatMain);
    assert_combat_ran(&table, prompts, 1);

    pass_to(&mut table, P0, Phase::Upkeep);
    assert_eq!(pending_skips(&table, P1), 0);
}

/// CR 614.10a: two skips cover P1's next two combat phases, one each.
#[test]
fn two_skips_cover_the_next_two_combat_phases() {
    let mut table = table();
    let skip = PendingCombatPhaseSkip {
        player: P1,
        duration: None,
    };
    table.runner.state_mut().pending_combat_phase_skips = vec![skip.clone(), skip];

    let life = (P0_STARTING_LIFE, P1_STARTING_LIFE);
    for remaining in [1, 0] {
        let prompts = play_p1_combat(&mut table);
        assert_combat_skipped(&table, prompts, life);
        assert_eq!(pending_skips(&table, P1), remaining);
        pass_to(&mut table, P0, Phase::End);
    }

    let prompts = play_p1_combat(&mut table);
    assert_combat_ran(&table, prompts, 1);
}

/// CR 616.1: the player chooses which of two skips applies; using the unlimited
/// one first leaves the limited one to expire (CR 514.2). The limited skip is
/// the older one so timestamp order would pick it.
#[test]
fn unlimited_skip_is_used_before_one_limited_to_this_turn() {
    let mut table = table();
    pass_to(&mut table, P1, Phase::PreCombatMain);
    table.runner.state_mut().pending_combat_phase_skips = vec![
        PendingCombatPhaseSkip {
            player: P1,
            duration: Some(Duration::UntilEndOfTurn),
        },
        PendingCombatPhaseSkip {
            player: P1,
            duration: None,
        },
    ];

    let life = (P0_STARTING_LIFE, P1_STARTING_LIFE);
    let prompts = pass_to(&mut table, P1, Phase::PostCombatMain);
    assert_combat_skipped(&table, prompts, life);
    assert_eq!(pending_skips(&table, P1), 1, "one skip used up");

    pass_to(&mut table, P0, Phase::Upkeep);
    let prompts = play_p1_combat(&mut table);
    assert_combat_ran(&table, prompts, 1);
}

/// CR 500.11: False Peace skips through the same transition authority, so
/// normal priority passing doesn't run its skipped steps either. CR 614.10a: a
/// one-shot skip then waits for the first combat that isn't skipped.
#[test]
fn turn_scoped_skip_comes_first_and_the_one_shot_waits_past_it() {
    let mut table = table();
    add_mana(&mut table.runner, P0, ManaType::Red, 1);
    assert!(cast_and_resolve(&mut table.runner, table.false_peace, P1));
    assert!(cast_patriarch(&mut table, 1, 4, P1));

    let life = (P0_STARTING_LIFE, P1_STARTING_LIFE);
    let prompts = play_p1_combat(&mut table);
    assert_combat_skipped(&table, prompts, life);
    assert_eq!(pending_skips(&table, P1), 1, "waits past False Peace");

    pass_to(&mut table, P0, Phase::End);
    let prompts = play_p1_combat(&mut table);
    assert_combat_skipped(&table, prompts, life);
    assert_eq!(pending_skips(&table, P1), 0);

    pass_to(&mut table, P0, Phase::End);
    let prompts = play_p1_combat(&mut table);
    assert_combat_ran(&table, prompts, 1);
}
