#!/usr/bin/env node
const [candidate, previous = ''] = process.argv.slice(2);
const parse = (value) => {
  const match = String(value).match(/^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/);
  if (!match) throw new Error(`version invalide : ${value || '(vide)'}`);
  return { numbers: match.slice(1, 4).map(Number), prerelease: match[4] || null };
};
if (!candidate) throw new Error('usage: assert-version-newer.mjs CANDIDATE [PREVIOUS]');
if (!previous) {
  parse(candidate);
  console.log(`[release] première version publiable : ${candidate}`);
  process.exit(0);
}
const left = parse(candidate);
const right = parse(previous);
let comparison = 0;
for (let index = 0; index < 3 && comparison === 0; index += 1) {
  comparison = Math.sign(left.numbers[index] - right.numbers[index]);
}
if (comparison === 0) {
  if (left.prerelease === null && right.prerelease !== null) comparison = 1;
  else if (left.prerelease !== null && right.prerelease === null) comparison = -1;
  else comparison = String(left.prerelease || '').localeCompare(String(right.prerelease || ''), 'en', { numeric: true });
}
if (comparison <= 0) throw new Error(`${candidate} doit être strictement supérieure à ${previous}`);
console.log(`[release] promotion autorisée : ${previous} -> ${candidate}`);
