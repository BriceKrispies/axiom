/*
 * workbench.ts — the configuration workbench. Edits a working draft of one
 * game's `CasinoGameConfig`: target win rate (slider + numeric), the reward
 * tier editor, presentation knobs, theme accent, the game-specific block, a
 * seed field with Preview, JSON import/export, and Restore Defaults. Every
 * change re-validates (shared validation + the game's `validateSpec`); errors
 * are readable and invalid drafts can be neither saved nor previewed.
 */

import type { CasinoGameConfig, Rarity, RewardKind, RewardTier } from "../chance-engine/configuration/schema.ts";
import { exportConfigJson, importConfigJson } from "../chance-engine/configuration/serialization.ts";
import type { ConfigIssue } from "../chance-engine/configuration/validation.ts";
import { validateConfig } from "../chance-engine/configuration/validation.ts";
import type { CasinoGameDefinition } from "../chance-engine/registry/definition.ts";
import type { BrandSpec, Rgb } from "../presentation/branding/brand.ts";
import { hexToRgb, readBrand, rgbToHex } from "../presentation/branding/brand.ts";
import { clearStoredConfig, storeConfig, storedConfigOf } from "../application/config-store.ts";

const RARITY_OPTIONS: readonly Rarity[] = ["common", "uncommon", "rare", "jackpot"];
const KIND_OPTIONS: readonly RewardKind[] = ["prize", "points", "tickets", "stars", "gems", "capsules", "toy", "retry"];
const THEME_PRESETS: readonly { readonly label: string; readonly accent: readonly [number, number, number] | null }[] = [
  { accent: null, label: "Pavilion (default)" },
  { accent: [1, 0.62, 0.35], label: "Sunset coral" },
  { accent: [0.45, 0.9, 0.72], label: "Fresh mint" },
  { accent: [0.72, 0.6, 0.98], label: "Lavender dusk" },
  { accent: [1, 0.83, 0.35], label: "Warm gold" },
];

type AnyConfig = CasinoGameConfig<unknown>;

export interface WorkbenchHandlers {
  readonly onPreview: (gameId: string, config: AnyConfig, seed: number | null) => void;
  readonly onClose: () => void;
}

export interface Workbench {
  readonly open: (definition: CasinoGameDefinition<unknown>) => void;
  readonly host: HTMLElement;
}

export const buildWorkbench = (host: HTMLElement, handlers: WorkbenchHandlers): Workbench => {
  let definition: CasinoGameDefinition<unknown> | null = null;
  let draft: AnyConfig | null = null;

  const validate = (config: AnyConfig): readonly ConfigIssue[] =>
    definition === null ? [] : [...validateConfig(config), ...definition.validateSpec(config.gameSpecific)];

  const render = (): void => {
    if (definition === null || draft === null) {
      return;
    }
    const def = definition;
    const config = draft;
    const issues = validate(config);
    host.replaceChildren();

    const heading = document.createElement("h2");
    heading.textContent = `Service Bay — ${def.displayName}`;
    host.append(heading);

    const errors = document.createElement("div");
    errors.id = "wb-errors";
    errors.textContent = issues.map((issue) => `• ${issue.path}: ${issue.message}`).join("\n");
    host.append(errors);

    const patch = (changes: Partial<AnyConfig>): void => {
      draft = { ...config, ...changes } as AnyConfig;
      render();
    };

    // ── payout calibration plate: win rate + reward tiers ─────────
    const tiersBox = document.createElement("fieldset");
    const tiersLegend = document.createElement("legend");
    tiersLegend.textContent = "Payout calibration plate";
    tiersBox.append(tiersLegend);

    const rateRow = document.createElement("div");
    rateRow.className = "row";
    const rateLabel = document.createElement("label");
    rateLabel.textContent = "Target win rate";
    const rateSlider = document.createElement("input");
    rateSlider.type = "range";
    rateSlider.min = "0";
    rateSlider.max = "1";
    rateSlider.step = "0.01";
    rateSlider.value = String(Number.isFinite(config.targetWinRate) ? config.targetWinRate : 0);
    const rateNumber = document.createElement("input");
    rateNumber.type = "number";
    rateNumber.min = "0";
    rateNumber.max = "1";
    rateNumber.step = "0.01";
    rateNumber.value = String(config.targetWinRate);
    rateSlider.addEventListener("input", () => patch({ targetWinRate: Number(rateSlider.value) }));
    rateNumber.addEventListener("change", () => patch({ targetWinRate: Number(rateNumber.value) }));
    rateRow.append(rateLabel, rateSlider, rateNumber);
    tiersBox.append(rateRow);
    const table = document.createElement("table");
    table.className = "cab-tier-table";
    const head = document.createElement("tr");
    for (const text of ["Tier id", "Label", "Rarity", "Weight", "Win?", "Reward kind", "Reward label", "Amount", ""]) {
      const th = document.createElement("th");
      th.textContent = text;
      head.append(th);
    }
    table.append(head);

    const patchTier = (index: number, changes: Partial<RewardTier>): void => {
      const tiers = config.rewardTiers.map((tier, i) => (i === index ? { ...tier, ...changes } : tier));
      patch({ rewardTiers: tiers });
    };

    config.rewardTiers.forEach((tier, index) => {
      const row = document.createElement("tr");
      const cell = (node: HTMLElement): void => {
        const td = document.createElement("td");
        td.append(node);
        row.append(td);
      };
      const text = (value: string, apply: (v: string) => void): HTMLInputElement => {
        const input = document.createElement("input");
        input.type = "text";
        input.value = value;
        input.addEventListener("change", () => apply(input.value));
        return input;
      };
      const num = (value: number, apply: (v: number) => void): HTMLInputElement => {
        const input = document.createElement("input");
        input.type = "number";
        input.step = "any";
        input.value = String(value);
        input.addEventListener("change", () => apply(Number(input.value)));
        return input;
      };
      cell(text(tier.id, (v) => patchTier(index, { id: v })));
      cell(text(tier.label, (v) => patchTier(index, { label: v })));
      const raritySelect = document.createElement("select");
      for (const rarity of RARITY_OPTIONS) {
        const option = document.createElement("option");
        option.value = rarity;
        option.textContent = rarity;
        option.selected = rarity === tier.rarity;
        raritySelect.append(option);
      }
      raritySelect.addEventListener("change", () => patchTier(index, { rarity: raritySelect.value as Rarity }));
      cell(raritySelect);
      cell(num(tier.weight, (v) => patchTier(index, { weight: v })));
      const winBox = document.createElement("input");
      winBox.type = "checkbox";
      winBox.checked = tier.countsAsWin;
      winBox.addEventListener("change", () => patchTier(index, { countsAsWin: winBox.checked }));
      cell(winBox);
      const kindSelect = document.createElement("select");
      for (const kind of KIND_OPTIONS) {
        const option = document.createElement("option");
        option.value = kind;
        option.textContent = kind;
        option.selected = kind === tier.reward.kind;
        kindSelect.append(option);
      }
      kindSelect.addEventListener("change", () =>
        patchTier(index, { reward: { ...tier.reward, kind: kindSelect.value as RewardKind } }),
      );
      cell(kindSelect);
      cell(text(tier.reward.label, (v) => patchTier(index, { reward: { ...tier.reward, label: v } })));
      cell(num(tier.reward.amount, (v) => patchTier(index, { reward: { ...tier.reward, amount: v } })));
      const remove = document.createElement("button");
      remove.textContent = "✕";
      remove.title = "Remove tier";
      remove.addEventListener("click", () => patch({ rewardTiers: config.rewardTiers.filter((_, i) => i !== index) }));
      cell(remove);
      table.append(row);
    });
    tiersBox.append(table);
    const addTier = document.createElement("button");
    addTier.textContent = "+ Add tier";
    addTier.addEventListener("click", () =>
      patch({
        rewardTiers: [
          ...config.rewardTiers,
          {
            countsAsWin: true,
            id: `tier-${config.rewardTiers.length + 1}`,
            label: "New Prize",
            rarity: "common",
            reward: { amount: 1, kind: "prize", label: "New Prize" },
            weight: 10,
          },
        ],
      }),
    );
    tiersBox.append(addTier);
    host.append(tiersBox);

    // ── presentation knobs ──────────────────────────────────────
    const knobs = document.createElement("fieldset");
    const knobsLegend = document.createElement("legend");
    knobsLegend.textContent = "Motor & lighting control section";
    knobs.append(knobsLegend);
    const sliderRow = (label: string, min: number, max: number, step: number, value: number, apply: (v: number) => void): HTMLElement => {
      const row = document.createElement("div");
      row.className = "row";
      const l = document.createElement("label");
      l.textContent = label;
      const slider = document.createElement("input");
      slider.type = "range";
      slider.min = String(min);
      slider.max = String(max);
      slider.step = String(step);
      slider.value = String(value);
      const out = document.createElement("input");
      out.type = "number";
      out.step = String(step);
      out.value = String(value);
      slider.addEventListener("input", () => apply(Number(slider.value)));
      out.addEventListener("change", () => apply(Number(out.value)));
      row.append(l, slider, out);
      return row;
    };
    knobs.append(
      sliderRow("Presentation speed", 0.25, 3, 0.05, config.presentationSpeed, (v) => patch({ presentationSpeed: v })),
      sliderRow("Celebration intensity", 0, 2, 0.05, config.celebrationIntensity, (v) => patch({ celebrationIntensity: v })),
    );
    if (config.choiceCount !== undefined) {
      knobs.append(
        sliderRow("Number of choices", 2, 24, 1, config.choiceCount, (v) => patch({ choiceCount: Math.round(v) })),
      );
    }
    const themeRow = document.createElement("div");
    themeRow.className = "row";
    const themeLabel = document.createElement("label");
    themeLabel.textContent = "Cabinet paint";
    const themeSelect = document.createElement("select");
    THEME_PRESETS.forEach((preset, i) => {
      const option = document.createElement("option");
      option.value = String(i);
      option.textContent = preset.label;
      const current = config.theme?.accent;
      option.selected = preset.accent === null ? current === undefined : JSON.stringify(preset.accent) === JSON.stringify(current);
      themeSelect.append(option);
    });
    themeSelect.addEventListener("change", () => {
      const preset = THEME_PRESETS[Number(themeSelect.value)];
      if (preset !== undefined) {
        patch({ theme: preset.accent === null ? undefined : { accent: preset.accent } });
      }
    });
    const motionLabel = document.createElement("label");
    motionLabel.textContent = "Reduced motion";
    const motionSelect = document.createElement("select");
    for (const mode of ["system", "on", "off"] as const) {
      const option = document.createElement("option");
      option.value = mode;
      option.textContent = mode;
      option.selected = config.reducedMotion === mode;
      motionSelect.append(option);
    }
    motionSelect.addEventListener("change", () => patch({ reducedMotion: motionSelect.value as AnyConfig["reducedMotion"] }));
    themeRow.append(themeLabel, themeSelect, motionLabel, motionSelect);
    knobs.append(themeRow);
    host.append(knobs);

    // ── brand controls (only for games whose gameSpecific carries a brand) ──
    const brand = readBrand(config.gameSpecific);
    if (brand !== null) {
      const brandConfig = config.gameSpecific as Record<string, unknown>;
      const patchBrand = (next: Partial<BrandSpec>): void => {
        patch({ gameSpecific: { ...brandConfig, brand: { ...brand, ...next } } } as Partial<AnyConfig>);
      };
      const brandBox = document.createElement("fieldset");
      const brandLegend = document.createElement("legend");
      brandLegend.textContent = "Brand livery";
      brandBox.append(brandLegend);

      const nameRow = document.createElement("div");
      nameRow.className = "row";
      const nameLabel = document.createElement("label");
      nameLabel.textContent = "Brand name";
      const nameInput = document.createElement("input");
      nameInput.type = "text";
      nameInput.id = "wb-brand-name";
      nameInput.value = brand.name;
      nameInput.addEventListener("change", () => patchBrand({ name: nameInput.value }));
      nameRow.append(nameLabel, nameInput);
      brandBox.append(nameRow);

      const colorRow = (label: string, value: Rgb, apply: (next: Rgb) => void): HTMLElement => {
        const row = document.createElement("div");
        row.className = "row";
        const swatchLabel = document.createElement("label");
        swatchLabel.textContent = label;
        const picker = document.createElement("input");
        picker.type = "color";
        picker.value = rgbToHex(value);
        picker.addEventListener("input", () => {
          const rgb = hexToRgb(picker.value);
          if (rgb !== null) {
            apply(rgb);
          }
        });
        row.append(swatchLabel, picker);
        return row;
      };
      brandBox.append(
        colorRow("Primary (banners, lettering)", brand.primary, (c) => patchBrand({ primary: c })),
        colorRow("Lettering on primary", brand.onPrimary, (c) => patchBrand({ onPrimary: c })),
        colorRow("Signboard ink", brand.ink, (c) => patchBrand({ ink: c })),
      );
      host.append(brandBox);
    }

    // ── game-specific block ─────────────────────────────────────
    const specBox = document.createElement("fieldset");
    const specLegend = document.createElement("legend");
    specLegend.textContent = "Mechanism configuration (JSON)";
    specBox.append(specLegend);
    const specArea = document.createElement("textarea");
    specArea.value = JSON.stringify(config.gameSpecific, null, 2);
    const specApply = document.createElement("button");
    specApply.textContent = "Apply game-specific JSON";
    specApply.addEventListener("click", () => {
      try {
        patch({ gameSpecific: JSON.parse(specArea.value) });
      } catch (error) {
        errors.textContent = `• gameSpecific: not valid JSON (${String(error)})`;
      }
    });
    specBox.append(specArea, specApply);
    host.append(specBox);

    // ── seed + preview ──────────────────────────────────────────
    const diagBox = document.createElement("fieldset");
    const diagLegend = document.createElement("legend");
    diagLegend.textContent = "Operator diagnostic readout";
    diagBox.append(diagLegend);
    const seedRow = document.createElement("div");
    seedRow.className = "row";
    const seedLabel = document.createElement("label");
    seedLabel.textContent = "Preview seed (blank = fresh)";
    const seedInput = document.createElement("input");
    seedInput.type = "text";
    seedInput.placeholder = "e.g. 12345";
    seedInput.id = "wb-seed";
    seedRow.append(seedLabel, seedInput);
    diagBox.append(seedRow);
    host.append(diagBox);

    // ── import / export ─────────────────────────────────────────
    const jsonBox = document.createElement("fieldset");
    const jsonLegend = document.createElement("legend");
    jsonLegend.textContent = "Service port (export / import)";
    jsonBox.append(jsonLegend);
    const jsonArea = document.createElement("textarea");
    jsonArea.id = "wb-json";
    jsonArea.value = exportConfigJson(config);
    const jsonActions = document.createElement("div");
    jsonActions.className = "actions";
    const exportBtn = document.createElement("button");
    exportBtn.textContent = "Export to text box";
    exportBtn.addEventListener("click", () => {
      jsonArea.value = exportConfigJson(config);
    });
    const importBtn = document.createElement("button");
    importBtn.textContent = "Import from text box";
    importBtn.addEventListener("click", () => {
      const result = importConfigJson<unknown>(jsonArea.value, def.id);
      if (result.config === null) {
        errors.textContent = result.issues.map((issue) => `• ${issue.path}: ${issue.message}`).join("\n");
        return;
      }
      const specIssues = def.validateSpec(result.config.gameSpecific);
      if (specIssues.length > 0) {
        errors.textContent = specIssues.map((issue) => `• ${issue.path}: ${issue.message}`).join("\n");
        return;
      }
      draft = result.config;
      render();
    });
    jsonActions.append(exportBtn, importBtn);
    jsonBox.append(jsonArea, jsonActions);
    host.append(jsonBox);

    // ── main actions ────────────────────────────────────────────
    const actions = document.createElement("div");
    actions.className = "actions";
    const preview = document.createElement("button");
    preview.className = "primary";
    preview.id = "wb-preview";
    preview.textContent = "Save & Preview";
    preview.disabled = issues.length > 0;
    preview.addEventListener("click", () => {
      if (validate(config).length === 0) {
        storeConfig(config);
        const seedText = (host.querySelector("#wb-seed") as HTMLInputElement).value.trim();
        const seed = seedText === "" ? null : Number(seedText) >>> 0;
        handlers.onPreview(def.id, config, seed);
      }
    });
    const save = document.createElement("button");
    save.textContent = "Save";
    save.disabled = issues.length > 0;
    save.addEventListener("click", () => {
      storeConfig(config);
      save.textContent = "Saved ✓";
      setTimeout(() => {
        save.textContent = "Save";
      }, 900);
    });
    const restore = document.createElement("button");
    restore.className = "danger";
    restore.textContent = "Restore defaults";
    restore.addEventListener("click", () => {
      clearStoredConfig(def.id);
      draft = def.defaultConfig();
      render();
    });
    const close = document.createElement("button");
    close.textContent = "Close";
    close.addEventListener("click", handlers.onClose);
    actions.append(preview, save, restore, close);
    host.append(actions);
  };

  return {
    host,
    open: (def): void => {
      definition = def;
      draft = storedConfigOf(def);
      render();
      host.classList.add("active");
      host.scrollIntoView({ behavior: "smooth" });
    },
  };
};
