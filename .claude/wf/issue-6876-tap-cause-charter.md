# Revised Phase Charter — Issue #6876 (Agent Maria Hill, cost-qualified tap trigger)

`BASE_SHA 0f1a35b4de154ca969ca2e8f9ed6db85225c7dc3` (verified `git rev-parse HEAD`).
Charter mode. **Revision round — decision revision.** Design frozen; not re-planned.

Absorbed: **B1**, **B2**, **R1**, **R2**, Corrections 1–3, and both residual assumptions
(each decided below, not deferred).

---

## 0. Step 0 — premise verification (survives into charter mode; re-run this round)

Scryfall `cards/named?exact=Agent Maria Hill`, fetched at this round:

> `Whenever Agent Maria Hill becomes tapped to pay a teamwork cost, put a +1/+1 counter on her and draw a card.`
> Legendary Creature — Human Spy Hero · `{W}` · MSH

Matches the issue body verbatim. `AdditionalCostOrigin::Teamwork` already exists
(`types/ability.rs`), so the Teamwork additional cost is landed infrastructure, not part of
this task. **Premise PASSES.**

CR numbers used below, each grep-verified against `docs/MagicCompRules.txt` this round:
`701.26` (Tap and Untap), `701.26a`, `508.1f` ("Tapping a creature when it's declared as an
attacker isn't a cost"), `601.2h`, `603.2`, `603.2a`, `702.51a` (Convoke), `702.122b` ("A
creature *crews a Vehicle* when it's tapped to pay the cost"), `702.154a` (Enlist),
`702.171c` ("A creature *saddles* a permanent as it's tapped to pay the cost").

CR 508.1f is the categorical warrant for the frozen shape: attacker-declaration tapping is
*not* a cost, so `AttackDeclaration` is a **sibling** of `CostPayment`, never a `TapCostKind`.
CR 702.122b / 702.171c define crew and saddle as *tapped to pay a cost*, which is what puts
`CrewFamily(CrewAction)` under `CostPayment`.

---

## 1. Frozen design (unchanged — restated so each phase plan inherits it verbatim)

- **REFUSE** `TriggerMode::TapsToPayTeamwork`. A per-mechanic trigger mode is a card-shaped
  variant; the cause belongs on the event, not on the mode.
- **CREATE** `TapCause { AttackDeclaration, CostPayment(TapCostKind), Effect { source } }`.
- **CREATE** `TapCostKind { TapSymbol, TapCreatures { origin: Option<AdditionalCostOrigin> },
  ManaShard(ConvokeMode), CrewFamily(CrewAction), Enlist, Harmonize }`.
- **Phase 1** — retype `PermanentTapped.caused_by` → `cause: TapCause`; stamp every producer;
  thread `PendingCast.paying_additional_cost_origin`; translate the opponent-tap gate in
  `match_taps`.
- **Phase 2** — `TriggerDefinition.tap_cause`; `match_taps` equality gate; parser; Hill tests.
- **Identity contract** — Hill fires **iff** `cause == TapCause::CostPayment(TapCostKind::TapCreatures
  { origin: Some(AdditionalCostOrigin::Teamwork) })`. The authority is the event's stamped cause,
  bound at tap time by the payment site, never re-derived at trigger time.
- Two phases, infrastructure then consumer.

---

## 2. Residual assumptions — decided

### RA-1. Does Phase 2 owe its own `PROTOCOL_VERSION` bump? → **No. Phase 2 carries no wire scope.**

Decided against a second bump, and the decision is claimed (**C2.5**), not asserted. Three
measured facts stand behind it:

1. `client/src/adapter/types.ts` types `GameObject.trigger_definitions` as `unknown[]`. The
   client provably never decodes a trigger body, so an additive field on `TriggerDefinition`
   changes nothing a client reads. (This is also the honest reading of B2 — see §Phase 2 scope.)
2. P2P is **hub-and-spoke: the host runs the authoritative engine and guests never construct
   the engine adapter** (`client/src/adapter/p2p-adapter.ts`). There is exactly one engine per
   game, so a Phase-1-build peer and a Phase-2-build peer cannot disagree about whether Hill's
   trigger fires. This is the fact that closes the only skew a no-bump decision would otherwise
   leave open, and it is why the answer differs from Phase 1's.
3. The v70 precedent named in `crates/server-core/src/protocol.rs`
   (`protocol_version_is_70_for_booster_pack_origin`) was a **required-field replacement inside
   a `WaitingFor` variant the client actively decodes**. `tap_cause` is optional, additive, and
   inside an opaque array. Bumping for it would inflate the version on a surface no peer can
   observe, and the convention the script encodes reserves the bump for shapes a peer must decode.

Phase 1 is the opposite case and does bump: `caused_by → cause` is a **rename plus retype** of a
payload field on `GameEvent`, a tagged union the Rust decoder deserializes into typed values on
both the full-game and P2P wires, where `#[serde(default)]` would silently degrade rather than
refuse.

### RA-2. Does Phase 2 need `ai_support/mod.rs`? → **No, and it is not in Phase 2 scope.** Claim **C2.6**.

`crates/engine/src/ai_support/mod.rs` has **no `TriggerMode::Taps` arm** — its beneficial-trigger
predictor enumerates `TapsForMana | ManaAbilityProduced | ManaAdded` and falls to `_ => false`.
It cannot over-predict a cost-qualified trigger because it does not predict `Taps` at all. The two
sites that *do* read `TriggerMode::Taps` are `crates/phase-ai/src/policies/self_untap_loop.rs`
(`has_tap_payoff_trigger`) and `crates/engine/src/analysis/ability_graph.rs` (`trigger_axis`); both
match on `mode` alone, both are unchanged by an additive field, and both already
over-approximate by design at base. Neither is in scope. C2.6 measures this rather than assuming it.

Note that `ai_support/mod.rs` **is** in **Phase 1** scope, for an unrelated and compiler-forced
reason: its mana probe constructs a `PermanentTapped` literal.

---

# Phase 1 — Stamp the cause on every tap

## Goal

Give every `PermanentTapped` event a typed, producer-stamped `TapCause`, replacing the
`Option<ObjectId> caused_by` field, and translate the existing opponent-tap gate in `match_taps`
onto the new type — **so that no trigger that fires today stops firing; the single intended new
firing is the C1.2 replacement-resume case.** No trigger gains a cost qualifier in this phase; no
card changes behavior except as C1.2 describes.

## Scope rule

Literal paths, no globs. The three standing inclusion classes (`/engine-implementer`:
compiler-forced sites, shared registration files, comment-only edits) are implied and not
enumerated. The orchestrator materializes `SCOPE_PATHS` at scope-freeze.

**Types**
- `crates/engine/src/types/events.rs` — the retype and the `TapCause` / `TapCostKind` definitions
- `crates/engine/src/types/game_state.rs` — `PendingCast.paying_additional_cost_origin`
- `crates/engine/src/types/proposed_event.rs` — **conditional, governed by C1.2.** `ProposedEvent::Tap`
  carries `{ object_id, applied }` and no source today; in scope only if C1.2 resolves to stamping
  `Effect { source }` at the replacement-resume site.

**Producers (stamp the cause)**
- `crates/engine/src/game/restrictions.rs` — `tap_permanent_for_cost`, the single cost-tap authority
- `crates/engine/src/game/effects/tap_untap.rs` — `Effect { source }`
- `crates/engine/src/game/combat.rs` — `AttackDeclaration` (CR 508.1f)
- `crates/engine/src/game/engine_replacement.rs` — the C1.2 resume site
- `crates/engine/src/game/casting_costs.rs` — producer, `pay_tap_creatures_selection`, and the origin threading
- `crates/engine/src/game/mana_sources.rs`
- `crates/engine/src/ai_support/mod.rs` — mana-probe literal

**Cost-payment callers (assign a `TapCostKind` — see C1.6)**
- `crates/engine/src/game/engine_combat.rs` — `apply_attack_enlist` → `Enlist`
- `crates/engine/src/game/engine_casting.rs` — `handle_harmonize_tap_choice` → `Harmonize`
- `crates/engine/src/game/costs.rs` — `pay_ability_cost_inner` → `TapSymbol`
- `crates/engine/src/game/mana_abilities.rs` — `tap_source` → `TapSymbol`; `tap_selected_creature_for_mana_cost` → **C1.6's named site**
- `crates/engine/src/game/engine.rs` — convoke → `ManaShard(ConvokeMode)`; crew / station / saddle → `CrewFamily(CrewAction)`

**Readers**
- `crates/engine/src/game/public_state.rs` — dirty-marking destructures the field by name
- `crates/engine/src/game/trigger_matchers.rs` — `match_taps`, the opponent-tap gate translation (C1.3)

**Wire** (B1's five paths, plus two authoritative homes B1's list omits — see C1.4)
- `client/src/adapter/ws-adapter.ts`
- `scripts/check-protocol-version.mjs`
- `crates/server-core/src/protocol.rs`
- `client/src/network/__tests__/protocol.test.ts` *(excluded from the T2 count — test)*
- `client/src/adapter/__tests__/p2p-adapter-multiplayer.test.ts` *(excluded from the T2 count — test)*
- `crates/lobby-broker/src/protocol.rs` — **planner addendum.** `pub const PROTOCOL_VERSION: u32 = 70`
  is authored here; `server-core` derives it. The bump is not performable without this path.
- `client/src/network/protocol.ts` — **planner addendum.** `export const WIRE_PROTOCOL_VERSION = 53 as const`
  is authored here; `check-protocol-version.mjs` reads it and refuses an unbumped P2P surface.
- `client/src/adapter/types.ts` — the `PermanentTapped` union arm (durable form per Correction 2 —
  the arm is named, not located: the member of the `GameEvent` discriminated union whose
  `type` is `"PermanentTapped"`)

**Advisory — touch only if the compiler or a mirror gate demands (C1.0)**
- `crates/engine/src/game/log.rs`, `crates/engine/src/game/trigger_index.rs`

## Claims Phase 1 must establish

Each is a claim with a measurement, never a statement of fact.

**C1.0 — the advisory list is advisory, and nothing forced hides outside the scope rule.**
Advisory (touch only if the compiler or a mirror gate demands): `crates/engine/src/game/log.rs`,
`crates/engine/src/game/trigger_index.rs` — each reads `PermanentTapped` through
`{ object_id, .. }` or `{ .. }` and so is not forced by the retype. Claim C1.0 — every remaining
site the retype forces is a `GameEvent::PermanentTapped` struct literal in a `#[cfg(test)]`
module, admitted at materialization as a compiler-forced path. Measurement: enumerate every
`GameEvent::PermanentTapped {` construction under `crates/engine/src` and `crates/engine/tests`
and confirm each forced site outside the scope rule is test-module-only; a forced construction in
a non-test module is a scope-rule finding, not a materialization detail.

**C1.1 — behavioral neutrality.** No trigger that fires at base stops firing after the retype.
Measurement: the existing engine suite is the instrument — `test-engine` green with no test
edited to accommodate a changed firing, and the tap-observer triggers in
`crates/engine/tests/integration/cost_zone_pipeline.rs` (the `TriggerMode::Taps` observers) pass
unmodified. A test that must be *relaxed* to stay green falsifies C1.1.

**C1.2 — the replacement-resume case is the single intended new firing.** The
`ProposedEvent::Tap` resume arm in `game/engine_replacement.rs` stamps `caused_by: None` at base,
which the opponent-tap gate reads as self-initiated and refuses. Measurement: determine from the
resume site whether a causing source is recoverable there; if it is, stamp `Effect { source }`
and show the one gate outcome that flips, with a paired positive reach-guard proving the fixture
reaches the resume arm and not an earlier tap path; if it is not recoverable without widening
`ProposedEvent::Tap`, the phase either widens it (admitting
`crates/engine/src/types/proposed_event.rs` per the conditional scope entry) or stamps the
conservative cause and records that no firing changes. Whichever way it resolves, the phase must
name the outcome and prove it; "the resume site is unreachable" is not admissible without a
reach-guard.

**C1.3 — the opponent-tap gate translates without changing its verdict.** CR 701.26 + CR 508.1f:
`match_taps` today gates a `controller: Some(Opponent)` `valid_card` on
`caused_by`'s controller, refusing `None` outright. Measurement: for each of the three base
verdicts — cause controlled by the trigger's controller (fire), cause controlled by another
(refuse), self-initiated (refuse) — exhibit the `TapCause` value the translated gate receives and
show the verdict is identical, with `AttackDeclaration` and each `CostPayment` variant covered as
the hostile siblings of the `Effect { source }` case.

**C1.4 — the wire bump is complete and every pin moves together.** Measurement:
`node scripts/check-protocol-version.mjs` exits 0 after the bump, exercising specifically —
(i) the two `EXPECTED_*` pins this change moves, `EXPECTED_PROTOCOL_VERSION` (70) in
`scripts/check-protocol-version.mjs` and `EXPECTED_WIRE_PROTOCOL_VERSION` (53) in the same file,
each of which must equal the value newly authored in `crates/lobby-broker/src/protocol.rs` and
`client/src/network/protocol.ts` respectively and mirrored in `client/src/adapter/ws-adapter.ts`;
and (ii) the `protocol_version_is_<n>` **require/refuse pair** in
`crates/server-core/src/protocol.rs` — the script requires a `fn protocol_version_is_<new>` and
refuses any surviving mention of `protocol_version_is_<new-1>`, so the test function must be
renamed rather than added alongside. The `AUTHORED_LITERALS` classifier additionally refuses a
bumped constant that has been re-derived into an expression; leave each authored constant a bare
integer. Frozen floors (`MIN_LOBBY_PROTOCOL_FOR_TOURNAMENT_ACK`,
`MIN_LOBBY_PROTOCOL_FOR_DEFAULT_SCORING`) and the lobby and directory versions must **not** move —
a diff that touches them falsifies C1.4.

**C1.5 — the origin threading has a single authority.** `PendingCast.paying_additional_cost_origin`
must be the only place a tap payment learns which additional cost it is paying, so
`TapCostKind::TapCreatures { origin }` is stamped once at the payment site rather than inferred by
any caller. Measurement: `pay_tap_creatures_selection` is the sole reader of the new field; show
that no call site of `tap_permanent_for_cost` inspects the pending cast to choose its variant.

**C1.6 — `TapCostKind` is total over `tap_permanent_for_cost` callers.**
`TapCostKind` must be total over `tap_permanent_for_cost` callers. Measurement: enumerate every
caller and assign each a variant before the signature change;
`mana_abilities::tap_selected_creature_for_mana_cost` (Springleaf Drum class) is the one site with
no eponymous variant and must be assigned explicitly. A site with no correct variant falsifies the
frozen `TapCostKind` shape and escalates rather than being absorbed into an adjacent variant.

*Planner note for the phase plan, not a substitute for the measurement:* the candidate assignment
for the named site is `TapCreatures { origin: None }` — it is a tap-creatures cost belonging to an
activated ability rather than a spell's additional cost, which is precisely the case
`Option<AdditionalCostOrigin>` exists to express. C1.6 is satisfied only if the phase plan
confirms this by enumeration and states it; if `origin: None` turns out to be indistinguishable
from a case that must be distinguishable, that is the escalation C1.6 names.

## Verification plan

Phase 1's discriminating test for the *task* is `DEFERRED(phase 2)` — no card observes a tap cause
until the consumer lands, which is the defining property of this seam. Phase 1's own verification
is structural plus unit-level:

- Green tree; existing suites unchanged and unrelaxed (C1.1).
- Tilt resources: `clippy`, `test-engine`, `card-data`, `check-frontend`, and **`test-frontend`**
  (added per B1 — the two protocol test files in the Wire group are vitest suites, so
  `check-frontend` alone cannot observe the bump).
- `node scripts/check-protocol-version.mjs` exits 0 (C1.4).
- Unit assertions at the seam: the `match_taps` translation table from C1.3, and the
  caller→variant enumeration from C1.6.
- `cargo fmt --all` run directly (Tilt does not auto-format).

## Deferral list

Everything the full task needs that Phase 1 intentionally omits, attributed to its landing phase:

- `TriggerDefinition.tap_cause` and the equality gate — **phase 2**
- Parsing "to pay a teamwork cost" — **phase 2**
- Coverage classifier honesty for unrecognized cost words — **phase 2**
- The Agent Maria Hill discriminating cast-pipeline test — **phase 2**
- Any second `PROTOCOL_VERSION` bump — **not owed**, decided at RA-1 and claimed at C2.5

## Recursive T1 ∧ T2

- **T1 (≥2 units): FALSE.** Phase 1 is one unit — a single typed-cause retype implementable by one
  skill-checklist pass. Its breadth is lockstep registration, not independently testable behaviors;
  it has no discriminating test of its own (see the seam note).
- **T2 (≥13 scope paths): TRUE.** See the enumeration below.
- **Conjunction: FALSE.** Phase 1 does not itself trip the gate; no further split. T2 alone never
  triggers a split.

---

# Phase 2 — Consume the cause on the trigger

## Goal

Let a triggered ability require a specific tap cause, and parse Agent Maria Hill's Teamwork
qualifier into it, so the trigger fires only on a Teamwork additional-cost tap — while any tap
qualifier the parser does not recognize leaves the card honestly unsupported rather than silently
producing an unqualified `Taps` trigger.

## Scope rule

**Engine**
- `crates/engine/src/types/ability.rs` — `TriggerDefinition.tap_cause: Option<TapCause>`, additive,
  `#[serde(default, skip_serializing_if = "Option::is_none")]`
- `crates/engine/src/game/trigger_matchers.rs` — the equality gate in `match_taps`
- `crates/engine/src/parser/oracle_trigger.rs` — the `SimpleEvent::BecomesTapped` arm
- `crates/engine/src/game/coverage.rs` — the classifier (B2)

**Frontend mirror** (B2)
- `client/src/adapter/types.ts` — in scope so the phase owns the decision; whether it is *edited*
  is decided by **C2.5**, whose measurement records that `GameObject.trigger_definitions` is typed
  `unknown[]` and therefore absorbs an additive field without a TS edit. A scope rule admits the
  path; the claim decides the diff.

**Tests** *(excluded from the T2 count)*
- `crates/engine/tests/integration/` — the Hill discriminating test (new file)
- `crates/engine/tests/integration/main.rs` — its `mod` line (shared registration file, standing class)
- `crates/engine/src/parser/oracle_trigger_tests.rs` — parser-level assertions

**Not in scope, by decision:** `crates/engine/src/ai_support/mod.rs` (RA-2 / C2.6);
`crates/phase-ai/src/policies/self_untap_loop.rs` and
`crates/engine/src/analysis/ability_graph.rs` (mode-only readers, unchanged by an additive field);
all wire paths (RA-1 / C2.5).

## Claims Phase 2 must establish

**C2.1 — the defect is live at base and this phase is what fixes it.** Measurement: at
`PHASE_BASE_SHA`, a cast-pipeline test in which Hill becomes tapped for a *non*-Teamwork reason
awards a `+1/+1` counter and a card; after the change it awards neither, while the Teamwork
payment path still awards both. Both legs in one test file, so the negative leg has its paired
positive reach-guard and cannot pass vacuously through an unparsed ability.

**C2.2 — the parser binds the qualifier, and binds it to the frozen identity.** Measurement:
`"becomes tapped to pay a teamwork cost"` yields `TriggerMode::Taps` with
`tap_cause == Some(CostPayment(TapCreatures { origin: Some(Teamwork) }))`, built with nom
combinators (`tag`/`alt`/`value` composed on the existing `SimpleEvent::BecomesTapped` arm — no
`contains`/`find`/`split_once` dispatch), and the sibling `"becomes tapped"` with no qualifier
still yields `tap_cause == None`. The Hill line must round-trip through the same
`parse_leading_turn_constraint` tail handling that arm already performs, so the Captain America
"during your turn" class does not regress.

**C2.3 — the gate is equality, and `None` is permissive.** Measurement: a trigger with
`tap_cause: None` fires on every `TapCause` (the base behavior every other tap trigger depends on);
a trigger with `tap_cause: Some(x)` fires on `x` and on nothing else. Hostile siblings:
`CostPayment(TapCreatures { origin: Some(Kicker) })`, `CostPayment(TapCreatures { origin: None })`,
`CostPayment(TapSymbol)`, `CrewFamily(_)`, and `AttackDeclaration` must each refuse against a
Teamwork-qualified trigger. `AttackDeclaration` is the load-bearing one: CR 508.1f makes attacker
tapping not a cost, so a card tapped by attacking must not satisfy a cost-qualified trigger.

**C2.4 — an unrecognized tap qualifier stays honestly unsupported.** Restated per B2 against the
`coverage.rs` classifier: today a trigger is unsupported only when its mode is
`TriggerMode::Unknown(_)`, its mode is absent from the trigger registry, or
`trigger_has_unimplemented_parts` holds; `TriggerMode::Taps` is registered, so a dropped qualifier
classifies as **supported** — which is why this issue is labelled a supported-aspect defect.
Measurement: a `"becomes tapped to pay a <qualifier the parser does not model>"` line must reach
red coverage or a strict-failure marker through `coverage.rs`'s classifier, and must **not** yield
a supported unqualified `Taps` trigger. Exhibit the classifier path that produces the red verdict
and a fixture whose coverage status is red before and after; a fixture that is red for an unrelated
reason does not buy this claim.

**C2.5 — no second wire bump is owed, and the TS mirror needs no edit.** Measurement:
`node scripts/check-protocol-version.mjs` exits 0 against a Phase-2 diff that touches no protocol
constant — its `EXPECTED_*` pins and its `protocol_version_is_<n>` require/refuse pair all read the
Phase-1 values and stay green, which is the script's own statement that this surface did not move.
Paired with the ws-adapter capability-bump convention: the client decodes
`GameObject.trigger_definitions` as `unknown[]`, so no client capability changes and no bump is
warranted; show that the TS diff for this phase is empty. If either leg fails, the wire paths from
Phase 1's Wire group are admitted to Phase 2 and the bump is taken.

**C2.6 — the AI does not over-predict a cost-qualified trigger.** Measurement: enumerate every
`TriggerMode::Taps` reader outside `trigger_matchers.rs` and show each is unchanged by an additive
field — `ai_support/mod.rs` has no `Taps` arm and falls to `_ => false` (so it predicts nothing to
over-predict); `phase-ai/src/policies/self_untap_loop.rs::has_tap_payoff_trigger` and
`analysis/ability_graph.rs::trigger_axis` match on `mode` alone and return the same verdict for a
Hill trigger before and after. If any reader's verdict changes, that path is admitted to scope and
the claim becomes a behavioral one requiring `cargo ai-gate` with a paired-seed report.

## Verification plan

- The Phase-1-deferred discriminating test lands here: the C2.1 cast-pipeline test, written to the
  `/card-test` recipe (`GameScenario` + `GameRunner::cast(..).resolve()` + `CastOutcome` deltas,
  verbatim Oracle text, no hand-built `TargetRef` vectors), in
  `crates/engine/tests/integration/` with its `mod` line added to `tests/integration/main.rs` —
  never a new top-level file under `crates/engine/tests/`.
- Parser assertions for C2.2 in `oracle_trigger_tests.rs`, including the unqualified sibling.
- The C2.3 hostile-sibling table as unit assertions at `match_taps`.
- The C2.4 coverage fixture, plus `cargo coverage` (a one-shot binary, not a Tilt resource) to
  confirm Agent Maria Hill's status moves for the right reason.
- Tilt: `clippy`, `test-engine`, `card-data`, `check-frontend`; `test-ai` because C2.6 asserts an
  unchanged AI verdict. `cargo fmt --all` direct.

## Deferral list

Nothing from the full task is deferred past Phase 2. Explicitly **not** owed, each decided rather
than deferred: a second `PROTOCOL_VERSION` bump (C2.5), `ai_support/mod.rs` (C2.6), and any
`TriggerMode` variant for Teamwork (refused in the frozen design).

## Recursive T1 ∧ T2

- **T1 (≥2 units): FALSE.** One unit — the cost-qualified tap trigger, one skill-checklist pass
  (`/add-trigger`) across its lockstep layers, one discriminating test.
- **T2 (≥13 scope paths): FALSE.** Five non-test paths after exclusions (`types/ability.rs`,
  `trigger_matchers.rs`, `oracle_trigger.rs`, `coverage.rs`, `client/src/adapter/types.ts`); test
  files and the integration `mod` line are excluded or grouped.
- **Conjunction: FALSE.** No further split.

---

# Seam notes

- **The seam is the event's cause field.** Phase 1 makes the cause representable and stamps it;
  Phase 2 makes a trigger able to require one. Phase 1 has no discriminating test of its own
  because nothing observes a cause until Phase 2 lands — that is the infrastructure→consumer edge
  T3 names as the preferred split point, and why Phase 1's verification is structural.
- **Shared files.** `crates/engine/src/game/trigger_matchers.rs` is touched by both phases —
  Phase 1 translates the opponent-tap gate onto `TapCause`, Phase 2 adds the equality gate beside
  it. They are separate edits to `match_taps` and must not be merged; Phase 2 must re-read the file
  rather than working from Phase 1's plan text.
  `crates/engine/tests/integration/main.rs` is the standing shared registration file for Phase 2's
  new test. `crates/engine/src/types/events.rs` and `crates/engine/src/types/ability.rs` are
  frequent multi-agent collision points — edits must be surgical.
- **Held green across the seam.** After Phase 1, every trigger's `tap_cause` is absent and
  `match_taps` ignores causes exactly as it does today, so the tree is green and no card's
  behavior has changed (C1.1) except the C1.2 case. Coverage stays honest across the seam because
  Phase 1 changes no card's parse: Agent Maria Hill remains wrongly-supported until Phase 2, which
  is the pre-existing state, not a regression Phase 1 introduces.
- **Advisory files are not a silent third scope.** `log.rs` and `trigger_index.rs` enter
  `SCOPE_PATHS` only through the compiler-forced standing class with the compiler error as
  evidence, never by planner fiat — that is what C1.0 exists to keep honest.

---

# Phase 1 scope-path enumeration (Correction 1 — enumerated honestly, and reconciled)

Counted under the phase-fit rule: test fixtures excluded outright; committed generated artifacts
and translation mirrors grouped with their authored source; directory entries expanded. **The
enumeration is the artifact; the integer is derived from it.** Regenerate with:

```
rg -n 'GameEvent::PermanentTapped\s*\{|tap_permanent_for_cost' crates/ client/ --glob '!target'
```

**Non-test, unconditional — 22:**

| # | Path | Group |
|---|---|---|
| 1 | `crates/engine/src/types/events.rs` | Types |
| 2 | `crates/engine/src/types/game_state.rs` | Types |
| 3 | `crates/engine/src/game/restrictions.rs` | Producer |
| 4 | `crates/engine/src/game/effects/tap_untap.rs` | Producer |
| 5 | `crates/engine/src/game/combat.rs` | Producer |
| 6 | `crates/engine/src/game/engine_replacement.rs` | Producer |
| 7 | `crates/engine/src/game/casting_costs.rs` | Producer + caller + origin |
| 8 | `crates/engine/src/game/mana_sources.rs` | Producer |
| 9 | `crates/engine/src/ai_support/mod.rs` | Producer |
| 10 | `crates/engine/src/game/engine_combat.rs` | Caller |
| 11 | `crates/engine/src/game/engine_casting.rs` | Caller |
| 12 | `crates/engine/src/game/costs.rs` | Caller |
| 13 | `crates/engine/src/game/mana_abilities.rs` | Caller (C1.6 site) |
| 14 | `crates/engine/src/game/engine.rs` | Caller ×4 |
| 15 | `crates/engine/src/game/public_state.rs` | Reader |
| 16 | `crates/engine/src/game/trigger_matchers.rs` | Reader (C1.3) |
| 17 | `client/src/adapter/types.ts` | Wire |
| 18 | `client/src/adapter/ws-adapter.ts` | Wire |
| 19 | `scripts/check-protocol-version.mjs` | Wire |
| 20 | `crates/server-core/src/protocol.rs` | Wire |
| 21 | `crates/lobby-broker/src/protocol.rs` | Wire (planner addendum) |
| 22 | `client/src/network/protocol.ts` | Wire (planner addendum) |

**Non-test, conditional — 1** (`crates/engine/src/types/proposed_event.rs`, admitted only if C1.2
resolves to threading a source). Upper bound **23**.

**Excluded tests — 2:** `client/src/network/__tests__/protocol.test.ts`,
`client/src/adapter/__tests__/p2p-adapter-multiplayer.test.ts`.

**Reconciliation with the correction's expected 24.** The correction anticipated *24 non-test + 2
excluded tests*; measurement at `BASE_SHA` yields **22 unconditional (23 with the C1.2
conditional) + 2 excluded tests**. The gap is accounted for and is not a missing scope path: R1, in
the same review, demoted `crates/engine/src/game/log.rs` and
`crates/engine/src/game/trigger_index.rs` from scope to advisory — exactly two paths — because each
reads `PermanentTapped` through `{ .. }` or `{ object_id, .. }` and is therefore not compiler-
forced. 24 is the pre-R1 figure; 22 is the same list after R1 is applied, and the two demoted paths
re-enter only through the compiler-forced standing class if the build demands them. Two further
candidates were measured and rejected rather than silently dropped:
`client/src/animation/types.ts` and `client/src/animation/eventNormalizer.ts` name
`"PermanentTapped"` only as a string key in an animation-duration map and a normalizer allowlist,
never reading the payload, so the retype does not force them.

**T2 verdict for Phase 1: TRUE** (22 ≥ 13). **T1: FALSE.** Conjunction **FALSE** — Correction 1's
recursive gate result is unchanged by the honest re-count, since 22, 23 and 24 all clear the same
threshold and T1 is what refuses.
