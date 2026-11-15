import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { OUTPUT, RATE, chime } from './generate-chime.mjs';

test('the committed chime is exactly what the script generates', () => {
  assert.ok(readFileSync(OUTPUT).equals(chime()), 'run: node scripts/generate-chime.mjs');
});

test('the chime is a 16-bit mono PCM WAV of 1.2 s that starts and ends silent and never clips', () => {
  const wav = chime();
  assert.equal(wav.toString('ascii', 0, 4), 'RIFF');
  assert.equal(wav.toString('ascii', 8, 16), 'WAVEfmt ');
  assert.deepEqual([wav.readUInt16LE(20), wav.readUInt16LE(22), wav.readUInt32LE(24), wav.readUInt16LE(34)], [1, 1, RATE, 16]);
  const samples = wav.readUInt32LE(40) / 2;
  assert.equal(samples, Math.round(RATE * 1.2));
  assert.equal(wav.length, 44 + samples * 2);
  const at = (i) => wav.readInt16LE(44 + i * 2);
  assert.equal(at(0), 0);
  assert.equal(at(samples - 1), 0);
  let peak = 0;
  for (let i = 0; i < samples; i++) peak = Math.max(peak, Math.abs(at(i)));
  assert.equal(peak, Math.round(0.8 * 32767));
});
