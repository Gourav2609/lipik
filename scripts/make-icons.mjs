// Draws the menu-bar icons from the Lipik mark and renders every icon PNG.
// Usage: node scripts/make-icons.mjs   (macOS 14+; run `bun run tauri icon src-tauri/icons/icon.png` after)
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");
const INK = "#285a44";
const LIVE = "#e0604f";

const mark = (color, bars = [4.6, 11.6, 7.4, 3]) => `
  <g stroke="${color}" stroke-linecap="round" fill="none">
    <path d="M3 5.4h18" stroke-width="2.6"/>
    ${bars.map((h, i) => `<path d="M${6.5 + 4 * i} 6.4v${h}" stroke-width="2.3"/>`).join("")}
  </g>`;
const SHORT = [2, 2, 2, 2];
const tile = (fill, inner) =>
  `<rect width="24" height="24" rx="6.5" fill="${fill}"/><g transform="translate(2.4 3.2) scale(0.8)">${inner}</g>`;
const dot = (fill, ring) =>
  `${ring ? `<circle cx="19" cy="19" r="4.6" fill="${ring}"/>` : ""}<circle cx="19" cy="19" r="3.2" fill="${fill}"/>`;
const alert = (fill, cut) =>
  `<circle cx="18.6" cy="18.6" r="5.4" fill="${cut}"/><circle cx="18.6" cy="18.6" r="4.4" fill="${fill}"/>
   <path d="M18.6 16.2v2.8" stroke="${cut}" stroke-width="1.6" stroke-linecap="round"/><circle cx="18.6" cy="21" r="0.9" fill="${cut}"/>`;

// One flat colour: menu-bar icons are drawn without cut-outs, which render
// blurry at 64 px.
const ring = (c) =>
  `<circle cx="18.6" cy="18.6" r="4.4" stroke="${c}" stroke-width="1.6" fill="none"/>
   <path d="M18.6 16.4v2.5" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/><circle cx="18.6" cy="20.8" r="0.9" fill="${c}"/>`;
const mono = (c) => ({
  idle: mark(c),
  recording: mark(c) + dot(c),
  transcribing: mark(c, SHORT),
  warning: mark(c) + ring(c),
});
const sets = {
  tray_: mono("#ffffff"),
  tray__dark: mono("#141210"),
};
const files = {};
for (const state of ["idle", "recording", "transcribing"]) {
  files[`tray_${state}.png`] = sets.tray_[state];
  files[`tray_${state}_dark.png`] = sets.tray__dark[state];
}
files["tray_idle_warning.png"] = sets.tray_.warning;
files["tray_idle_warning_dark.png"] = sets.tray__dark.warning;
files["handy.png"] = tile(INK, mark("#fbf9f4"));
files["recording.png"] = tile(INK, mark("#fbf9f4")) + dot(LIVE, INK);
files["transcribing.png"] = tile(INK, mark("#fbf9f4", SHORT));
files["handy_warning.png"] =
  tile(INK, mark("#fbf9f4")) + alert(LIVE, "#fbf9f4");

const dir = mkdtempSync(join(tmpdir(), "lipik-icons-"));
const render = (svg, size, out, view = 24) => {
  const file = join(dir, "i.svg");
  writeFileSync(
    file,
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${view} ${view}">${svg}</svg>`,
  );
  execFileSync(
    "swift",
    [join(ROOT, "scripts/render-svg.swift"), file, String(size), out],
    { stdio: "inherit" },
  );
};
for (const [name, svg] of Object.entries(files)) {
  render(svg, 64, join(ROOT, "src-tauri/resources", name));
}
const icon = readFileSync(
  join(ROOT, "src-tauri/icons/icon.svg"),
  "utf8",
).replace(/^<svg[^>]*>|<\/svg>\s*$/g, "");
render(icon, 1024, join(ROOT, "src-tauri/icons/icon.png"), 1024);
rmSync(dir, { recursive: true, force: true });
console.log(Object.keys(files).length + 1, "icons written");
