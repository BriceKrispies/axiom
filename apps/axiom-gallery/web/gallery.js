// Axiom app gallery — repo tooling (NOT part of the engine dependency graph;
// same status as the Makefile and scripts/). A plain ES module served
// statically; it imports nothing from the engine.
//
// There is NO app list in this file. Apps register themselves by carrying an
// `app.json` in their own directory; `scripts/package_gallery.py` discovers
// those at build time and writes `dist/manifest.json`, which this module
// fetches. Adding an app touches one file, inside that app's folder — and
// deleting the app folder removes it from the gallery with nothing to clean up
// here. (This file used to hold a hand-maintained DEMOS array, which is how
// five of its entries came to point at apps that no longer existed.)

/** Fetch the generated manifest. Served from the same directory as the page,
 * so the gallery works from any deploy sub-path. */
export const loadManifest = async () => {
  const response = await fetch("./manifest.json", { cache: "no-cache" });
  if (!response.ok) {
    throw new Error(`could not load manifest.json (HTTP ${response.status}) — run \`make gallery\` to generate it`);
  }
  return response.json();
};

/** Every distinct tag across the apps, in stable alphabetical order. */
export const tagsOf = (apps) => [...new Set(apps.flatMap((app) => app.tags ?? []))].sort();

/** The apps matching a free-text query and an optional tag, searched across
 * title, blurb, description, and tags. */
export const filterApps = (apps, { query = "", tag = null } = {}) => {
  const needle = query.trim().toLowerCase();
  return apps.filter((app) => {
    const matchesTag = tag === null || (app.tags ?? []).includes(tag);
    const haystack = [app.title, app.blurb, app.description, ...(app.tags ?? [])].join(" ").toLowerCase();
    return matchesTag && (needle === "" || haystack.includes(needle));
  });
};
