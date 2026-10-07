import { DEFAULT_MULTIPLAYER_SERVER_URL, serverHttpOrigin } from "../../config/multiplayerServer";
import { useMultiplayerStore } from "../../stores/multiplayerStore";
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
 */
export function resolvedEndpointOf(profile: LlmProfile): LlmEndpointConfig {
  const endpoint = endpointOf(profile);
  if (endpoint.provider !== "Jev" || endpoint.baseUrl?.trim()) return endpoint;
  return {
    ...endpoint,
    baseUrl:
      serverHttpOrigin(
        useMultiplayerStore.getState().hostingServer ?? DEFAULT_MULTIPLAYER_SERVER_URL,
      ) ?? null,
  };
}
