// Keys were named "flowkit.*" before the rename to Lipik. Imported first by
// every window so stores read the new keys on their first load.
const OLD_PREFIX = "flowkit.";

try {
  for (const old of Object.keys(localStorage)) {
    if (!old.startsWith(OLD_PREFIX)) continue;
    const key = `lipik.${old.slice(OLD_PREFIX.length)}`;
    const value = localStorage.getItem(old);
    if (value !== null && localStorage.getItem(key) === null) {
      localStorage.setItem(key, value);
    }
    localStorage.removeItem(old);
  }
} catch {
  // Storage can be unavailable; the stores fall back to their defaults.
}
