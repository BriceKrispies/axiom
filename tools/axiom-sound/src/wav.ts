// 16-bit PCM WAV encoding. The WAV is the tool's internal lossless intermediate
// (cached, never shipped): render produces Float32 channels, we validate them
// (pcm.ts), write a WAV, then FFmpeg encodes the WAV to the final MP3.

/** Per-channel Float32 sample data plus the sample rate. */
export interface RenderedPcm {
  readonly channels: readonly Float32Array[];
  readonly sampleRate: number;
}

const BYTES_PER_SAMPLE = 2; // 16-bit

/** Clamp a float sample to [-1, 1] and convert to signed 16-bit. */
function toInt16(sample: number): number {
  const clamped = sample < -1 ? -1 : sample > 1 ? 1 : sample;
  // Asymmetric full-scale mapping (standard): negative uses 0x8000, positive 0x7FFF.
  return clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff);
}

/**
 * Encode rendered Float32 channels into a canonical 16-bit PCM WAV byte buffer.
 * Channels are interleaved; all channels must be the same length.
 */
export function encodeWav(pcm: RenderedPcm): Buffer {
  const numChannels = pcm.channels.length;
  const numFrames = numChannels === 0 ? 0 : pcm.channels[0].length;
  const blockAlign = numChannels * BYTES_PER_SAMPLE;
  const dataSize = numFrames * blockAlign;
  const byteRate = pcm.sampleRate * blockAlign;

  const buffer = Buffer.alloc(44 + dataSize);
  buffer.write('RIFF', 0, 'ascii');
  buffer.writeUInt32LE(36 + dataSize, 4);
  buffer.write('WAVE', 8, 'ascii');
  buffer.write('fmt ', 12, 'ascii');
  buffer.writeUInt32LE(16, 16); // PCM fmt chunk size
  buffer.writeUInt16LE(1, 20); // audio format = PCM
  buffer.writeUInt16LE(numChannels, 22);
  buffer.writeUInt32LE(pcm.sampleRate, 24);
  buffer.writeUInt32LE(byteRate, 28);
  buffer.writeUInt16LE(blockAlign, 32);
  buffer.writeUInt16LE(8 * BYTES_PER_SAMPLE, 34); // bits per sample
  buffer.write('data', 36, 'ascii');
  buffer.writeUInt32LE(dataSize, 40);

  let offset = 44;
  for (let frame = 0; frame < numFrames; frame++) {
    for (let ch = 0; ch < numChannels; ch++) {
      buffer.writeInt16LE(toInt16(pcm.channels[ch][frame]), offset);
      offset += BYTES_PER_SAMPLE;
    }
  }
  return buffer;
}
