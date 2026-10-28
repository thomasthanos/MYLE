import assert from 'node:assert/strict';
import test from 'node:test';
import { appPresetIds, fromLegacy, inProfile, matchingProfile, pendingOf, profilePlan, prune, withProfile } from '../../frontend/app/pages/windows-optimization/debloat/selection.ts';
const tweak = (id, level, extra = {}) => ({ id, title: id, summary: '', category: 'privacy', risk: 'safe', level, note: null, restart: false, state: 'notApplied', canUndo: false, confirm: null, ...extra });
const tweaks = [
  tweak('telemetry', 'light'),
  tweak('bing', 'recommended'),
  tweak('location', 'maximum'),
  tweak('edge', null, { risk: 'caution' }),
  tweak('already', 'light', { state: 'applied' }),
  tweak('partial', 'recommended', { state: 'partial' }),
  tweak('unsupported', 'light', { state: 'unavailable' }),
  tweak('dark-mode', null, { state: 'applied' }),
];
const app = (id, level, installed = true) => ({ id, level, packages: installed ? [`Package.${id}`] : [] });
const apps = [app('candy', 'light'), app('clipchamp', 'recommended'), app('mail', 'maximum'), app('calculator', null), app('absent', 'light', false)];
const ids = (list) => list.map((item) => item.id);
test('each profile takes in the ones before it, and nothing without a level', () => {
  assert.ok(inProfile('light', 'light') && inProfile('light', 'maximum') && inProfile('recommended', 'maximum'));
  assert.ok(!inProfile('maximum', 'recommended') && !inProfile(null, 'maximum'));
});
test('a profile plans only what is not in place yet, on this PC', () => {
  assert.deepEqual(ids(profilePlan(tweaks, apps, 'light').tweaks), ['telemetry']);
  assert.deepEqual(ids(profilePlan(tweaks, apps, 'recommended').tweaks), ['telemetry', 'bing', 'partial']);
  assert.deepEqual(ids(profilePlan(tweaks, apps, 'maximum').tweaks), ['telemetry', 'bing', 'location', 'partial']);
  assert.deepEqual(ids(profilePlan(tweaks, apps, 'recommended').apps), ['candy', 'clipchamp']);
  assert.deepEqual(ids(profilePlan(tweaks, apps, 'maximum').apps), ['candy', 'clipchamp', 'mail']);
});
test('picking a profile adds, never turns off, and keeps what was picked by hand', () => {
  const desired = new Map([['edge', true], ['dark-mode', false], ['location', true]]);
  const picked = withProfile(tweaks, apps, desired, new Set(['calculator', 'mail']), 'light');
  // Location belonged to a profile, so Light decides it; Edge and Dark mode were picked by hand.
  assert.deepEqual([...picked.desired].sort(), [['dark-mode', false], ['edge', true], ['telemetry', true]]);
  assert.deepEqual([...picked.apps].sort(), ['calculator', 'candy']);
  assert.equal([...picked.desired.values()].filter((on) => !on).length, 1, 'only the hand-picked turn-off');
});
test('the picked profile is recognised, and a change by hand makes it custom', () => {
  const picked = withProfile(tweaks, apps, new Map(), new Set(), 'recommended');
  assert.equal(matchingProfile(tweaks, apps, picked.desired, picked.apps), 'recommended');
  picked.desired.delete('bing');
  assert.equal(matchingProfile(tweaks, apps, picked.desired, picked.apps), null);
  assert.equal(matchingProfile(tweaks, apps, new Map([['edge', true]]), new Set()), null, 'nothing from a profile');
});
test('pending changes are only those that change something', () => {
  const desired = new Map([['telemetry', true], ['already', true], ['dark-mode', false], ['bing', false], ['unsupported', true]]);
  const { on, off } = pendingOf(tweaks, desired);
  assert.deepEqual(ids(on), ['telemetry']);
  assert.deepEqual(ids(off), ['dark-mode']);
});
test('after a run, choices that are now how things are go away', () => {
  const after = tweaks.map((item) => (item.id === 'telemetry' ? { ...item, state: 'applied' } : item));
  const kept = prune(after, apps.map((item) => (item.id === 'candy' ? { ...item, packages: [] } : item)),
    new Map([['telemetry', true], ['bing', true], ['gone', true]]), new Set(['candy', 'clipchamp', 'absent']));
  assert.deepEqual([...kept.desired], [['bing', true]]);
  assert.deepEqual([...kept.apps], ['clipchamp']);
});
test('choices from the older version keep the apps picked by hand only', () => {
  const legacy = fromLegacy(['clipchamp', 'calculator']);
  assert.equal(legacy.desired.size, 0);
  assert.deepEqual([...legacy.apps], ['clipchamp', 'calculator']);
  assert.equal(fromLegacy(null).apps.size, 0);
});
test('app quick choices use installed packages and the profile levels', () => {
  assert.deepEqual(appPresetIds(apps, 'recommended'), ['candy', 'clipchamp']);
  assert.deepEqual(appPresetIds(apps, 'all'), ['candy', 'clipchamp', 'mail', 'calculator']);
  assert.deepEqual(appPresetIds(apps, 'none'), []);
});
