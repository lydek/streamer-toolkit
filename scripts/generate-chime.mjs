// Synthesises the default alert sound (a two-note chime) with Node's built-in modules only, so no third-party
// audio is bundled. `node scripts/generate-chime.mjs` rewrites apps/web/src/overlay/alerts/chime.wav; the test
// checks that the committed file is exactly what this produces.
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const OUTPUT = fileURLToPath(new URL('../apps/web/src/overlay/alerts/chime.wav', import.meta.url));
export const RATE = 22050;
const SECONDS = 1.2;
// Start (s) and pitch (Hz) of each note: A5, then E6.
const NOTES = [
  [0, 880],
  [0.12, 1318.51],
];
// Harmonic and its level, for a bell-like tone.
const PARTIALS = [
  [1, 1],
  [2, 0.3],
  [3, 0.1],
];

/** A 16-bit mono PCM WAV file. */
export function chime() {
  const n = Math.round(RATE * SECONDS);
  const samples = new Float64Array(n);
  for (const [start, freq] of NOTES) {
    for (let i = Math.round(start * RATE); i < n; i++) {
      const t = i / RATE - start;
      // 5 ms attack, then an exponential decay.
      const envelope = Math.min(1, t / 0.005) * Math.exp(-t / 0.3);
      let v = 0;
      for (const [harmonic, level] of PARTIALS) v += level * Math.sin(2 * Math.PI * freq * harmonic * t);
      samples[i] += envelope * v;
    }
  }
  const peak = samples.reduce((max, s) => Math.max(max, Math.abs(s)), 0);
  const wav = Buffer.alloc(44 + n * 2);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(36 + n * 2, 4);
  wav.write('WAVE', 8);
  wav.write('fmt ', 12);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20); // PCM
  wav.writeUInt16LE(1, 22); // mono
  wav.writeUInt32LE(RATE, 24);
  wav.writeUInt32LE(RATE * 2, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) {
    // The last 50 ms fade to silence, so the end never clicks; 80% of full scale leaves headroom.
    const fade = Math.min(1, (n - 1 - i) / (0.05 * RATE));
    wav.writeInt16LE(Math.round((samples[i] / peak) * 0.8 * fade * 32767), 44 + i * 2);
  }
  return wav;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  writeFileSync(OUTPUT, chime());
  console.log(`wrote ${OUTPUT}`);
}
