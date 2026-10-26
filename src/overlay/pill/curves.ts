// Timings and curves measured from screen recordings.

export type Curve = { ms: number; ease: string };

// Per-frame progress at 60 fps, played back exactly with CSS linear().
const frames = (progress: number[]): Curve => ({
  ms: Math.round(((progress.length - 1) * 1000) / 60),
  ease: `linear(${progress
    .map((p, i) => `${p} ${((i / (progress.length - 1)) * 100).toFixed(2)}%`)
    .join(", ")})`,
});

// Critically damped spring (mass 1, stiffness omega², damping 2·omega) from rest.
export const spring = (omega: number): Curve => {
  let end = 0;
  while (1 - (1 + omega * end) * Math.exp(-omega * end) < 0.998) end += 0.001;
  const steps = 24;
  const points = Array.from({ length: steps + 1 }, (_, i) => {
    const t = (end * i) / steps;
    const p = i === steps ? 1 : 1 - (1 + omega * t) * Math.exp(-omega * t);
    return `${p.toFixed(4)} ${((i / steps) * 100).toFixed(2)}%`;
  });
  return { ms: Math.round(end * 1000), ease: `linear(${points.join(", ")})` };
};

/** Underdamped spring (zeta < 1) from rest, sampled densely enough to keep its small overshoot. */
export const dampedSpring = (omega: number, zeta: number): Curve => {
  const wd = omega * Math.sqrt(1 - zeta * zeta);
  const x = (t: number) =>
    1 -
    Math.exp(-zeta * omega * t) *
      (Math.cos(wd * t) + ((zeta * omega) / wd) * Math.sin(wd * t));
  let end = 0.05;
  while (Math.abs(1 - x(end)) > 0.002 || Math.abs(1 - x(end + 0.02)) > 0.002)
    end += 0.001;
  const steps = 32;
  const points = Array.from({ length: steps + 1 }, (_, i) => {
    const p = i === steps ? 1 : x((end * i) / steps);
    return `${p.toFixed(4)} ${((i / steps) * 100).toFixed(2)}%`;
  });
  return { ms: Math.round(end * 1000), ease: `linear(${points.join(", ")})` };
};

export const bezier = (
  ms: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): Curve => ({
  ms,
  ease: `cubic-bezier(${x1}, ${y1}, ${x2}, ${y2})`,
});

/** Hover in/out and other pill size changes (S1 33.050, 112.217). */
export const PILL = frames([0, 0.39, 0.66, 0.82, 0.91, 0.95, 1]);
/** Idle -> fn recording pill, 40 -> 70 (S2 43.017). */
export const REC = frames([0, 0.633, 0.867, 0.9, 0.967, 1]);
/** Recording -> processing, 70 -> 95 (S2 38.867). */
export const PROC = frames([0, 0.4, 0.68, 0.76, 0.88, 0.96, 1]);
/** Done: processing 28x95 -> idle 8x40 via 14x54, 10x45, 8x41 (S2 18.85). */
export const DONE = frames([0, 0.72, 0.9, 0.98, 1]);

/** Notetaker pill in, 40 -> 55 -> 69 tall (notetaker1 65.083). */
export const NOTE_IN = frames([0, 0.52, 1]);
/** Notetaker pill out: width 30 -> 21 -> 18 -> 12 -> 8, height snaps (notetaker1 118.42). */
export const NOTE_OUT = frames([0, 0.41, 0.55, 0.82, 1]);
/** Notetaker hover ring, green -> #50514E (notetaker1 96.35, c1). */
export const NOTE_RING = bezier(100, 0.1, 0, 0.1, 1);

/** Toast growing out of the pill from a 23x6 bar (S1 39.80): width and height. */
export const TOAST_W = frames([
  0, 0.041, 0.063, 0.117, 0.273, 0.432, 0.572, 0.691, 0.782, 0.849, 0.892,
  0.928, 0.948, 0.964, 0.977, 0.986, 0.989, 0.993, 0.995, 0.998, 1,
]);
export const TOAST_H = frames([
  0, 0.024, 0.071, 0.235, 0.4, 0.55, 0.68, 0.78, 0.85, 0.89, 0.93, 0.95, 0.965,
  0.976, 0.988, 1,
]);
/** Surface shrinking back into the pill (S1 41.725). */
export const SHRINK = bezier(163, 0.2, 0.1, 0.4, 0.8);
/** Popup growing out of the pill (S2 19.958): width, then height/top. */
export const POPUP_W = bezier(263, 0.25, 0.3, 0.1, 1);
export const POPUP_H = bezier(213, 0.25, 0.5, 0.25, 0.9);
/** Popup shrinking back into the pill after its content faded (S2 25.233); it is gone by 83 ms. */
export const POPUP_EXIT = frames([0, 0.21, 0.3, 0.45, 0.58, 0.72, 0.86, 1]);
export const POPUP_EXIT_FADE = bezier(83, 0.4, 0, 1, 1);
/** Content fade + un-blur inside a grown surface (S2 20.317). */
export const REVEAL = bezier(297, 0.7, 0.8, 0.4, 1);
/** Permission card growing out of the pill (permissions1 26.09): width and height springs. */
export const PERM_W = dampedSpring(22.5, 0.9);
export const PERM_H = spring(27.5);
/** Its content fading in, opacity only (permissions1 26.50, c2). */
export const PERM_REVEAL = bezier(217, 0.3, 0.3, 0.6, 0.9);
/** Its surface settling 345x150 -> 340x148 during the reveal. */
export const PERM_SETTLE = bezier(130, 0.3, 0, 0.3, 1);
/** Tooltip sliding between buttons (S1 34.934). */
export const TIP_SLIDE = bezier(165, 0.25, 0.1, 0.25, 1);
/** Drag release: circle flies to the slot (S3 298.40, 303.68). */
export const SNAP = spring(30);

export const t = (prop: string, c: Curve, delay = 0) =>
  `${prop} ${c.ms}ms ${c.ease}${delay ? ` ${delay}ms` : ""}`;
