// SPDX-License-Identifier: GPL-3.0-or-later

export type MathOperation =
  | { kind: "derivative" }
  | { kind: "movingAverage"; window: number }
  | { kind: "lowPass"; alpha: number }
  | { kind: "gate"; min: number; max: number };

export interface MathChannelSpec {
  id: string;
  name: string;
  source: string;
  operation: MathOperation;
}

export type NumericColumn = readonly (number | null)[];

const finite = (value: number | null | undefined): value is number =>
  value !== null && value !== undefined && Number.isFinite(value);

export function derivative(
  values: NumericColumn,
  tMs: NumericColumn,
): (number | null)[] {
  const output = new Array<number | null>(values.length).fill(null);
  for (let i = 1; i < values.length; i += 1) {
    const value = values[i];
    const previous = values[i - 1];
    const time = tMs[i];
    const previousTime = tMs[i - 1];
    if (
      finite(value) &&
      finite(previous) &&
      finite(time) &&
      finite(previousTime) &&
      time > previousTime
    ) {
      output[i] = (value - previous) / ((time - previousTime) / 1000);
    }
  }
  return output;
}

export function movingAverage(
  values: NumericColumn,
  requestedWindow: number,
): (number | null)[] {
  const window = Math.max(1, Math.floor(requestedWindow));
  const output = new Array<number | null>(values.length).fill(null);
  const queue: number[] = [];
  let sum = 0;
  for (let i = 0; i < values.length; i += 1) {
    const value = values[i];
    if (!finite(value)) {
      queue.length = 0;
      sum = 0;
      continue;
    }
    queue.push(value);
    sum += value;
    if (queue.length > window) sum -= queue.shift() ?? 0;
    output[i] = sum / queue.length;
  }
  return output;
}

export function lowPass(
  values: NumericColumn,
  requestedAlpha: number,
): (number | null)[] {
  const alpha = Math.min(1, Math.max(0, requestedAlpha));
  const output = new Array<number | null>(values.length).fill(null);
  let previous: number | null = null;
  for (let i = 0; i < values.length; i += 1) {
    const value = values[i];
    if (!finite(value)) {
      previous = null;
      continue;
    }
    previous =
      previous === null ? value : alpha * value + (1 - alpha) * previous;
    output[i] = previous;
  }
  return output;
}

export function gate(
  values: NumericColumn,
  min: number,
  max: number,
): (number | null)[] {
  const lower = Math.min(min, max);
  const upper = Math.max(min, max);
  return values.map((value) =>
    finite(value) && value >= lower && value <= upper ? value : null,
  );
}

export function evaluateMathChannel(
  spec: MathChannelSpec,
  values: NumericColumn,
  tMs: NumericColumn,
): (number | null)[] {
  switch (spec.operation.kind) {
    case "derivative":
      return derivative(values, tMs);
    case "movingAverage":
      return movingAverage(values, spec.operation.window);
    case "lowPass":
      return lowPass(values, spec.operation.alpha);
    case "gate":
      return gate(values, spec.operation.min, spec.operation.max);
  }
}
