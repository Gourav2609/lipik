import assert from "node:assert/strict";
import {
  DEFAULT_MODEL_ID,
  EUROPEAN_MODEL_ID,
  modelForLanguage,
  WORLD_MODEL_ID,
} from "./speechLanguage";

const model = (id: string, languages: string[]) => ({
  id,
  supported_languages: languages,
});
const models = [
  model(DEFAULT_MODEL_ID, ["en"]),
  model(EUROPEAN_MODEL_ID, ["fr", "de", "es"]),
  model(WORLD_MODEL_ID, ["fr", "ja", "hi", "ta"]),
];

const cases: [string | null, string][] = [
  [null, DEFAULT_MODEL_ID],
  ["en-US", DEFAULT_MODEL_ID],
  ["en-IN", DEFAULT_MODEL_ID],
  ["hi-IN", DEFAULT_MODEL_ID],
  ["hi", DEFAULT_MODEL_ID],
  ["ta-IN", DEFAULT_MODEL_ID],
  ["ur", DEFAULT_MODEL_ID],
  ["bn-BD", DEFAULT_MODEL_ID],
  ["fr-IN", DEFAULT_MODEL_ID],
  ["fr-FR", EUROPEAN_MODEL_ID],
  ["de_DE", EUROPEAN_MODEL_ID],
  ["ja-JP", WORLD_MODEL_ID],
];
for (const [tag, want] of cases) {
  assert.equal(modelForLanguage(tag, models), want, String(tag));
}
assert.equal(
  modelForLanguage("ja-JP", [model(DEFAULT_MODEL_ID, ["en"])]),
  DEFAULT_MODEL_ID,
);
console.log("speechLanguage: ok");
