/**
 * The consecutive-failure breaker shared by LLM game seats and LLM drafters.
 *
 * Without one, a provider that is down, rate-limited or simply hanging costs
 * the full request timeout on EVERY decision, turning one misconfiguration into
 * a game or draft that stalls on each move. A tripped seat keeps playing — it
 * just uses the engine AI, which is what it was already falling back to.
 *
 * What a failure is charged to decides whether the breaker is fair, and it is
 * the pair (profile revision, seat):
 *
 *  - **Per seat.** Several seats can share one profile, and each makes its own
 *    request with its own prompt. One seat's usable reply says nothing about a
 *    sibling that keeps timing out, so a shared count would let the healthy
 *    seat reset the failing one on every round and the failing one would never
 *    reach the ceiling.
 *  - **Per profile revision, not per profile id.** The count belongs to the
 *    configuration that failed. `llmStore.updateProfile` replaces the edited
 *    record and leaves every other record referentially unchanged, so a record
 *    IS a committed revision: fixing a key, switching model, or toggling the
 *    profile off and on yields a new record and a fresh start, and rebinding a
 *    seat to another profile consults that profile's own history. Keying on the
 *    record (weakly) rather than on a projection of its fields also keeps the
 *    credential out of any key this module builds.
 *
 * A failure that is not attributable to one seat — the engine could not build
 * any request for the profile at all — is charged to the profile as a whole
 * (`seat` = `null`) and, once tripped, stops every seat on that revision.
 */

import type { LlmProfile } from "./types";

/**
 * Consecutive failures a (profile revision, seat) may take before the session
 * stops calling the provider for it.
 */
export const MAX_CONSECUTIVE_LLM_FAILURES = 3;

/**
 * The seat a failure is charged to: a seat number, or `null` for a failure of
 * the profile as a whole that no single seat's request produced.
 */
export type LlmBreakerSeat = number | null;

export class LlmSeatBreaker {
  private failures = new WeakMap<LlmProfile, Map<LlmBreakerSeat, number>>();

  /**
   * @param onTrip called once, when a seat (or the whole profile) first reaches
   *   the ceiling. Callers log a Phase-authored notice; no provider text is
   *   passed through here.
   */
  constructor(private readonly onTrip: (seat: LlmBreakerSeat, failures: number) => void) {}

  /** Whether this revision has been given up on for `seat` — or as a whole. */
  isTripped(profile: LlmProfile, seat: LlmBreakerSeat): boolean {
    const counts = this.failures.get(profile);
    if (!counts) return false;
    const tripped = (key: LlmBreakerSeat) =>
      (counts.get(key) ?? 0) >= MAX_CONSECUTIVE_LLM_FAILURES;
    return tripped(null) || (seat != null && tripped(seat));
  }

  /**
   * Record one outcome. A success clears that seat's count; a failure adds to
   * it. Each seat is judged only by its own outcomes.
   */
  record(profile: LlmProfile, seat: LlmBreakerSeat, succeeded: boolean): void {
    let counts = this.failures.get(profile);
    if (succeeded) {
      counts?.delete(seat);
      return;
    }
    if (!counts) {
      counts = new Map();
      this.failures.set(profile, counts);
    }
    const failures = (counts.get(seat) ?? 0) + 1;
    counts.set(seat, failures);
    if (failures === MAX_CONSECUTIVE_LLM_FAILURES) this.onTrip(seat, failures);
  }

  /** Forget every count, e.g. when a new draft starts. */
  reset(): void {
    this.failures = new WeakMap();
  }
}
