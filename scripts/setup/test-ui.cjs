#!/usr/bin/env node
const assert = require('node:assert/strict');
const selection = require('../../setup-ui/engine-selection.js');

const engines = [
  { id: 'ffmpeg', installed: true },
  { id: 'zstd', installed: false },
  { id: 'lz4', installed: false },
];

assert.deepEqual(selection.toggleAll(engines, []), ['ffmpeg', 'zstd', 'lz4']);
assert.deepEqual(selection.toggleAll(engines, ['ffmpeg', 'zstd', 'lz4']), []);
assert.deepEqual(selection.selectMissingByDefault(engines, []), ['zstd', 'lz4']);
assert.deepEqual(selection.selectMissingByDefault([
  { id: 'ffmpeg', installed: true },
  { id: 'zstd', installed: true },
], []), ['ffmpeg', 'zstd']);
assert.deepEqual(selection.selectMissingByDefault(engines, ['ffmpeg']), ['ffmpeg']);
assert.deepEqual(selection.summarize(engines, ['ffmpeg', 'zstd']), {
  selected: 2,
  total: 3,
  missing: 1,
  allSelected: false,
});

console.log('[setup-ui] sélection totale, désélection et moteurs manquants vérifiés');
