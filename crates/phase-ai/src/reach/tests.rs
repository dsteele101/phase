use engine::ai_support::flat_priority_actions;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::TargetRef;
use engine::types::actions::GameAction;
use engine::types::game_state::{GameState, StackEntryKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::mana::{ManaColor, ManaCost, ManaCostShard};
use engine::types::phase::Phase;
use rand::rngs::SmallRng;
use rand::SeedableRng;

use super::{find_lethal_line, lethal_priority_action, lethal_prompt_action, LineStep};
use crate::config::{create_config, AiDifficulty, Platform};

const SEAL_OF_FIRE: &str = "Sacrifice this enchantment: It deals 2 damage to any target.";
const FLAME_RIFT: &str = "Flame Rift deals 4 damage to each player.";
const LIGHTNING_BOLT: &str = "Lightning Bolt deals 3 damage to any target.";
const LAVA_SPIKE: &str = "Lava Spike deals 3 damage to target player or planeswalker.";
const BLAZE: &str = "Blaze deals X damage to any target.";
const GUTTERSNIPE: &str =
    "Whenever you cast an instant or sorcery spell, this creature deals 2 damage to each opponent.";
const BOROS_CHARM: &str = "Choose one —\n\
    • Boros Charm deals 4 damage to target player or planeswalker.\n\
    • Permanents you control gain indestructible until end of turn.\n\
    • Target creature gains double strike until end of turn.";

fn red(generic: u32, red_pips: usize) -> ManaCost {
    ManaCost::Cost {
        shards: vec![ManaCostShard::Red; red_pips],
        generic,
    }
}

/// A two-player game in the AI's precombat main phase with `mountains`
/// untapped Mountains and the opponent at `opponent_life`.
fn scenario(mountains: usize, opponent_life: i32) -> GameScenario {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_life(P1, opponent_life);
    for _ in 0..mountains {
        scenario.add_basic_land(P0, ManaColor::Red);
    }
    scenario
}

fn seal_in_hand(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_spell_to_hand(P0, "Seal of Fire", false)
        .as_enchantment()
        .with_mana_cost(red(0, 1))
        .from_oracle_text(SEAL_OF_FIRE)
        .id()
}

fn seal_on_battlefield(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_enchantment_from_oracle(P0, "Seal of Fire", SEAL_OF_FIRE)
        .with_mana_cost(red(0, 1))
        .id()
}

fn flame_rift(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_spell_to_hand_from_oracle(P0, "Flame Rift", false, FLAME_RIFT)
        .with_mana_cost(red(1, 1))
        .id()
}

fn lightning_bolt(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_spell_to_hand_from_oracle(P0, "Lightning Bolt", true, LIGHTNING_BOLT)
        .with_mana_cost(red(0, 1))
        .id()
}

fn lava_spike(scenario: &mut GameScenario) -> ObjectId {
    scenario
        .add_spell_to_hand_from_oracle(P0, "Lava Spike", false, LAVA_SPIKE)
        .with_mana_cost(red(0, 1))
        .id()
}

/// The admission rule the real decision boundary applies (targeted-exchange
/// gate plus the AI loop guards).
fn production_admission(state: &GameState, action: &GameAction) -> bool {
    crate::search::root_action_is_admitted(state, P0, action)
}

fn admitted_actions(state: &GameState) -> Vec<GameAction> {
    flat_priority_actions(state)
        .into_iter()
        .filter(|action| production_admission(state, action))
        .collect()
}

fn cast_spell(spell: ObjectId, state: &GameState) -> GameAction {
    GameAction::CastSpell {
        object_id: spell,
        card_id: state.objects[&spell].card_id,
        targets: Vec::new(),
        payment_mode: engine::types::game_state::CastPaymentMode::Auto,
    }
}

/// Cast `spell` through the reducer with the opponent as its target, leaving
/// it (and anything it triggers) on the stack.
fn cast_at_opponent(runner: &mut GameRunner, spell: ObjectId) {
    let cast = cast_spell(spell, runner.state());
    runner.act(cast).expect("the spell is castable");
    runner
        .act(GameAction::ChooseTarget {
            target: Some(TargetRef::Player(P1)),
        })
        .expect("the opponent is a legal target");
}

fn line(state: &GameState) -> Option<Vec<LineStep>> {
    let issued = flat_priority_actions(state);
    find_lethal_line(state, P0, &issued, &|_, _| true).map(|line| line.steps)
}

fn casts(steps: &[LineStep]) -> Vec<ObjectId> {
    steps
        .iter()
        .filter_map(|step| match step.action {
            GameAction::CastSpell { object_id, .. } => Some(object_id),
            _ => None,
        })
        .collect()
}

/// Let the AI play its turn against an opponent who only ever passes, and
/// report whether the AI won before the turn ended.
fn ai_wins_this_turn(runner: &mut GameRunner, difficulty: AiDifficulty) -> bool {
    let config = create_config(difficulty, Platform::Native);
    let mut rng = SmallRng::seed_from_u64(7);
    let turn = runner.state().turn_number;
    for _ in 0..80 {
        let state = runner.state();
        if let WaitingFor::GameOver { winner } = state.waiting_for {
            return winner == Some(P0);
        }
        if state.turn_number != turn {
            return false;
        }
        let action = if state.waiting_for.acting_player() == Some(P0) {
            crate::search::choose_action(state, P0, &config, &mut rng)
                .expect("the AI owes this decision")
        } else {
            GameAction::PassPriority
        };
        runner.act(action).expect("the chosen action applies");
    }
    false
}

/// The reported defect: two Seals look cheaper one at a time, but only
/// Seal + Flame Rift reaches 6.
#[test]
fn seal_seal_rift_takes_the_lethal_pair_not_the_cheap_pair() {
    let mut scenario = scenario(3, 6);
    let seal_a = seal_in_hand(&mut scenario);
    let seal_b = seal_in_hand(&mut scenario);
    let rift = flame_rift(&mut scenario);
    let runner = scenario.build();

    let steps = line(runner.state()).expect("Seal + Flame Rift is lethal with three lands");
    let cast = casts(&steps);
    assert!(
        cast.contains(&rift),
        "the line must cast Flame Rift: {steps:?}"
    );
    assert!(
        !(cast.contains(&seal_a) && cast.contains(&seal_b)),
        "two Seals deal 4 and cost the mana Flame Rift needs: {steps:?}"
    );
}

#[test]
fn seal_seal_rift_is_played_out_to_a_win() {
    let mut scenario = scenario(3, 6);
    seal_in_hand(&mut scenario);
    seal_in_hand(&mut scenario);
    flame_rift(&mut scenario);
    let mut runner = scenario.build();

    assert!(
        ai_wins_this_turn(&mut runner, AiDifficulty::Medium),
        "the AI must burn the opponent out this turn"
    );
}

/// A Seal already on the battlefield is damage that costs no mana: it is
/// folded into the total before any new spell is bought.
#[test]
fn battlefield_seal_is_free_reach() {
    let mut scenario = scenario(1, 5);
    let seal = seal_on_battlefield(&mut scenario);
    let bolt = lightning_bolt(&mut scenario);
    let mut runner = scenario.build();

    let steps = line(runner.state()).expect("Bolt (3) + the free Seal (2) is lethal");
    assert_eq!(casts(&steps), vec![bolt]);
    assert!(
        steps.iter().any(|step| matches!(
            step.action,
            GameAction::ActivateAbility { source_id, .. } if source_id == seal
        )),
        "the line must sacrifice the Seal already in play: {steps:?}"
    );
    assert!(ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

#[test]
fn battlefield_seal_alone_finishes_without_mana() {
    let mut scenario = scenario(0, 2);
    let seal = seal_on_battlefield(&mut scenario);
    let runner = scenario.build();

    let issued = flat_priority_actions(runner.state());
    let action = lethal_priority_action(runner.state(), P0, &issued, &|_, _| true);
    assert!(
        matches!(action, Some(GameAction::ActivateAbility { source_id, .. }) if source_id == seal),
        "the free Seal is the whole line: {action:?}"
    );
}

#[test]
fn no_line_when_reach_falls_short() {
    let mut scenario = scenario(1, 5);
    lightning_bolt(&mut scenario);
    let runner = scenario.build();

    assert!(line(runner.state()).is_none());
}

/// CR 104.4a: Flame Rift taking both players to 0 is a draw, not a win.
#[test]
fn mutual_destruction_is_not_lethal() {
    let mut scenario = scenario(2, 4);
    scenario.with_life(P0, 4);
    flame_rift(&mut scenario);
    let runner = scenario.build();

    assert!(line(runner.state()).is_none());
}

/// CR 107.3a: an X burn spell is cast last and sized to the mana the rest of
/// the line leaves over.
#[test]
fn x_burn_spell_absorbs_the_leftover_mana() {
    let mut scenario = scenario(5, 6);
    let bolt = lightning_bolt(&mut scenario);
    let blaze = scenario
        .add_spell_to_hand_from_oracle(P0, "Blaze", false, BLAZE)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::X, ManaCostShard::Red],
            generic: 0,
        })
        .id();
    let mut runner = scenario.build();

    let steps = line(runner.state()).expect("Bolt (3) + Blaze for X=3 is lethal");
    assert_eq!(casts(&steps), vec![bolt, blaze]);
    assert!(ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

/// CR 702.11c: a player with hexproof can't be targeted by a burn spell, but
/// non-targeted damage still reaches them.
#[test]
fn player_hexproof_leaves_only_untargeted_reach() {
    let mut scenario = scenario(3, 4);
    scenario.add_enchantment_from_oracle(P1, "Leyline of Sanctity", "You have hexproof.");
    let bolt = lightning_bolt(&mut scenario);
    let rift = flame_rift(&mut scenario);
    let runner = scenario.build();

    let steps = line(runner.state()).expect("Flame Rift needs no target");
    let cast = casts(&steps);
    assert_eq!(cast, vec![rift]);
    assert!(!cast.contains(&bolt));
}

/// A burn spell that is not lethal on its own still goes to the face when the
/// rest of the line finishes the job.
#[test]
fn line_member_targets_the_opponent_over_a_creature() {
    let mut scenario = scenario(1, 5);
    seal_on_battlefield(&mut scenario);
    let bolt = lightning_bolt(&mut scenario);
    scenario.add_creature(P1, "Goblin Guide", 2, 2);
    let mut runner = scenario.build();

    let card_id = runner.state().objects[&bolt].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: bolt,
            card_id,
            targets: Vec::new(),
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("Bolt is castable");
    assert!(matches!(
        runner.state().waiting_for,
        WaitingFor::TargetSelection { .. }
    ));

    let issued: Vec<GameAction> = engine::ai_support::build_decision_context(runner.state())
        .candidates
        .into_iter()
        .map(|candidate| candidate.action)
        .collect();
    let answer = lethal_prompt_action(runner.state(), P0, &issued, &|_, _| true);
    assert_eq!(
        answer,
        Some(GameAction::ChooseTarget {
            target: Some(TargetRef::Player(P1)),
        })
    );
}

#[test]
fn sorcery_burn_pair_is_played_out_to_a_win() {
    let mut scenario = scenario(2, 6);
    lava_spike(&mut scenario);
    lava_spike(&mut scenario);
    scenario.add_creature(P1, "Goblin Guide", 2, 2);
    let mut runner = scenario.build();

    assert!(ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

/// CR 700.2a: a modal burn spell is cast for its damaging mode.
#[test]
fn modal_burn_spell_is_cast_for_its_damage_mode() {
    let mut scenario = scenario(1, 4);
    scenario.add_basic_land(P0, ManaColor::White);
    scenario
        .add_spell_to_hand_from_oracle(P0, "Boros Charm", true, BOROS_CHARM)
        .with_mana_cost(ManaCost::Cost {
            shards: vec![ManaCostShard::Red, ManaCostShard::White],
            generic: 0,
        });
    scenario.add_creature(P1, "Goblin Guide", 2, 2);
    let mut runner = scenario.build();

    let steps = line(runner.state()).expect("the 4-damage mode is lethal");
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].mode, Some(0), "the damage mode is the first mode");
    assert!(ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

/// CR 602.2b: a mana-only activated ability is repeatable for as long as the
/// mana lasts, so two activations count toward the total.
#[test]
fn repeatable_activation_is_counted_per_activation() {
    let mut scenario = scenario(4, 2);
    let rod = scenario
        .add_artifact_from_oracle(
            P0,
            "Ember Rod",
            "{1}{R}: Ember Rod deals 1 damage to any target.",
        )
        .id();
    let mut runner = scenario.build();

    let steps = line(runner.state()).expect("two activations deal 2");
    assert_eq!(steps.len(), 2);
    assert!(steps.iter().all(|step| matches!(
        step.action,
        GameAction::ActivateAbility { source_id, .. } if source_id == rod
    )));
    assert!(ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

/// The line obeys the same admission rule as the real decision boundary at
/// every step it simulates: after two Lava Spikes this turn the third cast is
/// admitted and the fourth is refused (the search's same-card cast cap), so two
/// more Spikes against an opponent at 6 are not a lethal line.
#[test]
fn line_respects_the_same_card_cast_cap_at_every_step() {
    let mut scenario = scenario(4, 12);
    let spikes: Vec<ObjectId> = (0..4).map(|_| lava_spike(&mut scenario)).collect();
    let mut runner = scenario.build();
    for &spike in &spikes[..2] {
        cast_at_opponent(&mut runner, spike);
        runner.advance_until_stack_empty();
    }
    assert_eq!(
        runner.life(P1),
        6,
        "two Spikes resolved through the reducer"
    );

    let state = runner.state().clone();
    let admitted = admitted_actions(&state);
    assert!(
        admitted.contains(&cast_spell(spikes[2], &state))
            && admitted.contains(&cast_spell(spikes[3], &state)),
        "both remaining Spikes are admitted now: only two have been cast"
    );
    // Reach guard: without the admission rule, the two remaining Spikes would
    // certify — the certificate depends on the fourth cast.
    let permissive = flat_priority_actions(&state);
    assert!(find_lethal_line(&state, P0, &permissive, &|_, _| true).is_some());
    assert!(find_lethal_line(&state, P0, &admitted, &production_admission).is_none());
    assert_eq!(
        lethal_priority_action(&state, P0, &admitted, &production_admission),
        None
    );

    // The reducer agrees: the third cast is admitted, the fourth is not.
    let mut third = GameRunner::from_state(state.clone());
    cast_at_opponent(&mut third, spikes[2]);
    third.advance_until_stack_empty();
    assert!(flat_priority_actions(third.state()).contains(&cast_spell(spikes[3], third.state())));
    assert!(!production_admission(
        third.state(),
        &cast_spell(spikes[3], third.state())
    ));

    // And the chooser, playing the turn out, cannot burn the opponent out.
    let mut played = GameRunner::from_state(state);
    assert!(!ai_wins_this_turn(&mut played, AiDifficulty::Medium));
}

/// A Lightning Bolt cast with Guttersnipe in play, both still on the stack:
/// the Bolt and the Guttersnipe trigger above it are pending, and a second
/// Bolt is in hand with one Mountain to cast it.
fn bolt_and_guttersnipe_trigger_on_stack(opponent_life: i32) -> GameRunner {
    let mut scenario = scenario(2, opponent_life);
    // Summoning sick, so the line is about burn alone: Guttersnipe can't add
    // combat damage this turn (CR 508.1a).
    let guttersnipe = scenario
        .add_creature_from_oracle(P0, "Guttersnipe", 2, 2, GUTTERSNIPE)
        .with_mana_cost(red(2, 1))
        .with_summoning_sickness()
        .id();
    let first = lightning_bolt(&mut scenario);
    lightning_bolt(&mut scenario);
    let mut runner = scenario.build();
    cast_at_opponent(&mut runner, first);

    let state = runner.state();
    assert!(
        matches!(state.waiting_for, WaitingFor::Priority { player } if player == P0),
        "the AI holds priority with its Bolt on the stack"
    );
    assert!(
        state
            .stack
            .iter()
            .any(|entry| entry.source_id == guttersnipe
                && matches!(entry.kind, StackEntryKind::TriggeredAbility { .. })),
        "Guttersnipe's cast trigger is on the stack: {:?}",
        state.stack
    );
    runner
}

/// Pending stack work counts toward reach even when no card the AI holds
/// defines it: Guttersnipe's trigger (2) + the Bolt it rides on (3) + the
/// second Bolt (3) is 8 against 7 — before its own new trigger.
#[test]
fn pending_trigger_on_the_stack_counts_toward_lethal() {
    let mut runner = bolt_and_guttersnipe_trigger_on_stack(7);
    let state = runner.state().clone();
    let admitted = admitted_actions(&state);
    assert!(
        find_lethal_line(&state, P0, &admitted, &production_admission).is_some(),
        "the stack-aware line must not be pruned before it settles"
    );
    assert!(ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

/// Everything available — both Bolts and both Guttersnipe triggers — is 10,
/// short of 11.
#[test]
fn pending_trigger_does_not_invent_lethal() {
    let mut runner = bolt_and_guttersnipe_trigger_on_stack(11);
    let state = runner.state().clone();
    let admitted = admitted_actions(&state);
    assert!(find_lethal_line(&state, P0, &admitted, &production_admission).is_none());
    assert!(!ai_wins_this_turn(&mut runner, AiDifficulty::Medium));
}

/// `VeryEasy` does not plan multi-action plays.
#[test]
fn very_easy_does_not_commit_to_lines() {
    let config = create_config(AiDifficulty::VeryEasy, Platform::Native);
    assert!(!config.play_lookahead);
}
