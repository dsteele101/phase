import { useLlmStore } from "../../stores/llmStore";
import { useMultiplayerStore } from "../../stores/multiplayerStore";
import { defaultJevRelayOrigin } from "./relayOrigin";
import { endpointOf, type LlmEndpointConfig, type LlmProfile } from "./types";

/**
 * The endpoint the engine is handed for a profile.
 *
 * Jev is not called directly: TypeSafe's API refuses browser CORS, so its
 * requests go through a phase-server relay. A Jev profile with no endpoint of
 * its own uses the multiplayer server the player chose (or this build's
 * default), which is known here and not to the engine. Every other provider is passed through as
 * saved; the engine decides what each endpoint means and refuses one it cannot
 * use.
 *
 * A derived relay is resolved against the key the store holds NOW, not the one
 * on the profile copy the caller carries. Callers (a draft round, a probe) hold
 * their copy across awaits, and the store drops a key the moment its relay
 * origin changes (`llmStore`), so a copy taken under server A must not carry A's
 * key to server B. A profile the store no longer holds has no key at all.
 */
export function resolvedEndpointOf(profile: LlmProfile): LlmEndpointConfig {
  const endpoint = endpointOf(profile);
  if (endpoint.provider !== "Jev" || endpoint.baseUrl?.trim()) return endpoint;
  const current = useLlmStore.getState().profiles.find((candidate) => candidate.id === profile.id);
  return {
    ...endpoint,
    apiKey: current?.apiKey ?? "",
    baseUrl: defaultJevRelayOrigin(useMultiplayerStore.getState().hostingServer) ?? null,
  };
}
