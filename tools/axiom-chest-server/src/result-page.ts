/*
 * result-page.ts — the server-rendered result document.
 *
 * This is what a browser with JavaScript switched off gets back when it submits
 * the chest form. It is the BASELINE, so it is written to the baseline's rules:
 *
 *   - it must read correctly with the stylesheet stripped out, which is why the
 *     content is a heading, a definition list and a table rather than a pile of
 *     divs whose meaning lives in CSS;
 *   - the only way back into the game is a real `<form method="POST">` with a
 *     real `type="submit"` control, because a link cannot deal a new round and a
 *     JS handler is not available to us here;
 *   - every interpolated value is escaped. The board labels come from config,
 *     not from the player, but a result page that escapes "only where it has to"
 *     is one edit away from not escaping where it does.
 *
 * The stylesheet is the SAME `/styles/resilient.css` the form page links, and
 * this document uses the same class names, so tier 2 (CSS, no JS) looks like the
 * game rather than like an error page.
 */

import type { PickResponse, RevealedChest } from "../../../apps/casino-games/web/src/resilient/contract.ts";
import { NEW_ROUND_ENDPOINT } from "../../../apps/casino-games/web/src/resilient/contract.ts";

export const escapeHtml = (value: string): string =>
  value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");

const chestRow = (chest: RevealedChest, picked: number): string => {
  const yours = chest.index === picked;
  const label = chest.reward === null ? "empty" : `${chest.reward.tierLabel} — ${chest.reward.rewardLabel}`;
  const rarity = chest.reward === null ? "—" : chest.reward.rarity;
  return [
    `<tr${yours ? ' class="is-yours"' : ""}>`,
    `<th scope="row">Chest ${chest.index + 1}${yours ? " (yours)" : ""}</th>`,
    `<td>${escapeHtml(label)}</td>`,
    `<td>${escapeHtml(rarity)}</td>`,
    "</tr>",
  ].join("");
};

const document_ = (title: string, body: string): string =>
  [
    "<!doctype html>",
    '<html lang="en">',
    "<head>",
    '<meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    `<title>${escapeHtml(title)}</title>`,
    '<link rel="stylesheet" href="/styles/resilient.css">',
    "</head>",
    '<body class="resilient-body resilient-body--result">',
    body,
    "</body>",
    "</html>",
  ].join("\n");

/** The full result document for a resolved pick. */
export const renderResultPage = (result: PickResponse): string => {
  const headline = result.won ? "You won!" : "Empty chest";
  const prize =
    result.reward === null
      ? "That chest was empty."
      : `${result.reward.tierLabel} — ${result.reward.rewardLabel} (${result.reward.rarity}).`;

  const body = [
    '<main class="resilient-main">',
    "<h1>Treasure Chest Pick</h1>",
    `<h2 class="resilient-headline ${result.won ? "is-win" : "is-loss"}">${escapeHtml(headline)}</h2>`,
    `<p class="resilient-prize">You opened <strong>chest ${result.picked + 1}</strong>. ${escapeHtml(prize)}</p>`,
    result.replay
      ? '<p class="resilient-note">This round was already decided — you are seeing the recorded result, not a new one.</p>'
      : "",
    "<h3>The whole board</h3>",
    "<p>The prizes were placed before you picked, so here is what every chest held.</p>",
    '<table class="resilient-board">',
    "<thead><tr><th scope=\"col\">Chest</th><th scope=\"col\">Held</th><th scope=\"col\">Rarity</th></tr></thead>",
    "<tbody>",
    result.board.map((chest) => chestRow(chest, result.picked)).join("\n"),
    "</tbody>",
    "</table>",
    "<h3>Round facts</h3>",
    '<dl class="resilient-facts">',
    `<dt>Seed</dt><dd>${escapeHtml(String(result.seed))}</dd>`,
    `<dt>Round</dt><dd>${escapeHtml(String(result.round))}</dd>`,
    `<dt>Winning chests</dt><dd>${escapeHtml(String(result.winnerCount))} of ${escapeHtml(String(result.chestCount))}</dd>`,
    `<dt>Target win rate</dt><dd>${escapeHtml(String(Math.round(result.targetWinRate * 100)))}%</dd>`,
    "</dl>",
    `<form class="resilient-again" method="POST" action="${NEW_ROUND_ENDPOINT}">`,
    '<button class="resilient-submit" type="submit">Play another round</button>',
    "</form>",
    '<p><a href="/resilient.html">Back to the chests</a></p>',
    "</main>",
  ].join("\n");

  return document_(`${headline} — Treasure Chest Pick`, body);
};

/** A refusal, rendered for the same baseline audience. */
export const renderErrorPage = (message: string): string =>
  document_(
    "That pick did not work — Treasure Chest Pick",
    [
      '<main class="resilient-main">',
      "<h1>That pick did not work</h1>",
      `<p class="resilient-note">${escapeHtml(message)}</p>`,
      '<p><a href="/resilient.html">Back to the chests</a></p>',
      "</main>",
    ].join("\n"),
  );
