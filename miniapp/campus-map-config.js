/**
 * Campus map runtime configuration.
 *
 * Keep this file free of secrets in Git. It is intentionally a no-key default
 * so the local map continues to work until an owner supplies an authorised,
 * HTTP-referrer-restricted Google Maps Platform key.
 *
 * Optional runtime value:
 * window.CAMPUS_MAP_CONFIG = {
 *   googlePhotorealistic3dTilesKey: "YOUR_RESTRICTED_BROWSER_KEY"
 * };
 */
window.CAMPUS_MAP_CONFIG = window.CAMPUS_MAP_CONFIG || {
  googlePhotorealistic3dTilesKey: ""
};
