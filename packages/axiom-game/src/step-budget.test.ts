import assert from "node:assert/strict";
import { test } from "node:test";

import { type StepBudget, interpolationAlpha } from "./step-budget.ts";

const budget = (steps: number, remainderNanos: number, fixedStepNanos: number): StepBudget => ({
  fixedStepNanos,
  remainderNanos,
  steps,
});

test("interpolationAlpha is the banked remainder over the fixed step", () => {
  assert.equal(interpolationAlpha(budget(3, 250, 1000)), 0.25);
});

test("interpolationAlpha is 0 when nothing is banked", () => {
  assert.equal(interpolationAlpha(budget(1, 0, 1000)), 0);
});

test("interpolationAlpha approaches 1 just below a full step", () => {
  assert.equal(interpolationAlpha(budget(0, 999, 1000)), 0.999);
});
