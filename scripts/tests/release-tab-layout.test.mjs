// The Release tab fits the app's default window (its body is about 586×320 px at 1280×720): what keeps it compact, checked on the component's source. (Measured in Chromium while it was made: the form fits with no scroll bar, also with two failing checks shown.)
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
const source = await readFile(new URL("../../frontend/app/pages/github-releases/ReleaseTab.svelte", import.meta.url), "utf8");
const markup = source.slice(source.indexOf("</script>"), source.indexOf("<style>"));
const style = source.slice(source.indexOf("<style>"));
test("the notes take the height left instead of a fixed tall box", () => {
  assert.doesNotMatch(markup, /rows="6"/);
  assert.doesNotMatch(style, /\.notes \{[^}]*min-height: 110px/);
  assert.match(style, /\.notes \{ flex: 1;/);
  assert.match(style, /\.notes-section \{[^}]*flex: 1 1 0/);
});
test("how to release and what to build are compact switches, not cards", () => {
  assert.match(markup, /role="radiogroup" aria-label="How to release"/);
  assert.match(markup, /role="radiogroup" aria-label="What the workflows build"/);
  assert.doesNotMatch(markup, /class="modes"/);
});
test("the version files, commits and checks open from summaries", () => {
  assert.match(markup, /class="summary"[^>]*aria-haspopup="dialog"/);
  assert.match(markup, /class="summary checks-chip"/);
  assert.match(markup, /checks OK/);
  // A failing check is never hidden behind the chip alone.
  assert.match(markup, /class="checks inline-checks"/);
});
test("while the workflow runs, stopping is two clear actions", () => {
  assert.match(markup, /Stop following/);
  assert.match(markup, /Cancel run on GitHub/);
  assert.match(source, /title: "Cancel the run on GitHub\?"/, "cancelling on GitHub asks first");
});
test("failures come first in the run view and are brought into view", () => {
  const failures = markup.indexOf('class="failures"');
  const steps = markup.indexOf('<ol class="steps">');
  assert.ok(failures > 0 && failures < steps, "failure cards before the steps");
  assert.match(source, /scrollIntoView\(/);
  assert.match(style, /\.failures \{[^}]*overflow: auto/);
});
