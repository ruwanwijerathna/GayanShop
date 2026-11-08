// Generates the till's audio cues (doc §1.9) as small 16-bit mono WAVs in src/assets/audio/.
// Deterministic: re-running produces identical files. Run with `node scripts/gen-audio.mjs`.
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const RATE = 22050;
const OUT = join(process.cwd(), "src", "assets", "audio");

/** A list of [frequencyHz | 0 for silence, durationMs] segments → PCM samples with 5 ms fades. */
function synth(segments, gain = 0.5) {
  const samples = [];
  for (const [freq, ms] of segments) {
    const n = Math.round((RATE * ms) / 1000);
    const fade = Math.min(Math.round(RATE * 0.005), Math.floor(n / 2));
    for (let i = 0; i < n; i += 1) {
      const env = Math.min(1, i / fade, (n - 1 - i) / fade);
      samples.push(freq === 0 ? 0 : Math.sin((2 * Math.PI * freq * i) / RATE) * gain * env);
    }
  }
  return samples;
}

function wav(samples) {
  const data = Buffer.alloc(samples.length * 2);
  samples.forEach((s, i) =>
    data.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(s * 32767))), i * 2),
  );
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVE", 8);
  header.write("fmt ", 12);
  header.writeUInt32LE(16, 16); // PCM chunk size
  header.writeUInt16LE(1, 20); // PCM
  header.writeUInt16LE(1, 22); // mono
  header.writeUInt32LE(RATE, 24);
  header.writeUInt32LE(RATE * 2, 28); // byte rate
  header.writeUInt16LE(2, 32); // block align
  header.writeUInt16LE(16, 34); // bits per sample
  header.write("data", 36);
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

const CUES = {
  tick: [[880, 40]], // item added
  error: [
    [220, 80],
    [0, 40],
    [220, 80],
  ], // unknown barcode
  approval: [[660, 90]], // approval required
  complete: [
    [660, 80],
    [990, 110],
  ], // sale complete (rising)
  warning: [[440, 150]], // printer / system problem
};

mkdirSync(OUT, { recursive: true });
for (const [name, segments] of Object.entries(CUES)) {
  writeFileSync(join(OUT, `${name}.wav`), wav(synth(segments)));
}
process.stdout.write(`gen-audio: wrote ${Object.keys(CUES).length} cues to src/assets/audio\n`);
