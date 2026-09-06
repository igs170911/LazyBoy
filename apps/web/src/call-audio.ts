export const VOICE_SAMPLE_RATE = 24000;

export function floatToPcm16(input: Float32Array): Uint8Array {
  const pcm = new Int16Array(input.length);
  for (let i = 0; i < input.length; i += 1) {
    const sample = Math.max(-1, Math.min(1, input[i] ?? 0));
    pcm[i] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
  }
  return new Uint8Array(pcm.buffer);
}

export function pcm16ToFloat(bytes: ArrayBuffer): Float32Array {
  const pcm = new Int16Array(bytes.byteLength % 2 === 0 ? bytes : bytes.slice(0, bytes.byteLength - 1));
  const out = new Float32Array(pcm.length);
  for (let i = 0; i < pcm.length; i += 1) {
    out[i] = (pcm[i] ?? 0) / 32768;
  }
  return out;
}

export function resample(input: Float32Array, fromRate: number, toRate: number): Float32Array {
  if (fromRate === toRate || input.length === 0) return input;
  const ratio = fromRate / toRate;
  const length = Math.max(1, Math.round(input.length / ratio));
  const out = new Float32Array(length);
  for (let i = 0; i < length; i += 1) {
    const src = i * ratio;
    const left = Math.floor(src);
    const frac = src - left;
    const a = input[left] ?? 0;
    const b = input[Math.min(left + 1, input.length - 1)] ?? 0;
    out[i] = a + (b - a) * frac;
  }
  return out;
}

const WORKLET = `
class CaptureProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel && channel.length) this.port.postMessage(channel);
    return true;
  }
}
registerProcessor("lazyboy-capture", CaptureProcessor);
`;

export type CallAudioHandlers = {
  onCapture: (pcm: Uint8Array) => void;
  onError: (message: string) => void;
};

export class CallAudio {
  private stopped = false;
  private context: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private worklet: AudioWorkletNode | null = null;
  private nextTime = 0;
  private playing: AudioBufferSourceNode[] = [];
  private handlers: CallAudioHandlers;

  constructor(handlers: CallAudioHandlers) {
    this.handlers = handlers;
  }

  async start(): Promise<void> {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
    if (this.stopped) {
      stream.getTracks().forEach((track) => track.stop());
      return;
    }
    this.stream = stream;
    const context = new AudioContext();
    this.context = context;
    if (context.state === "suspended") await context.resume();
    const blob = new Blob([WORKLET], { type: "application/javascript" });
    const url = URL.createObjectURL(blob);
    try {
      await context.audioWorklet.addModule(url);
    } finally {
      URL.revokeObjectURL(url);
    }
    if (this.stopped) return;
    const source = context.createMediaStreamSource(stream);
    const worklet = new AudioWorkletNode(context, "lazyboy-capture");
    worklet.port.onmessage = (event) => {
      const samples = event.data as Float32Array;
      const resampled = resample(samples, context.sampleRate, VOICE_SAMPLE_RATE);
      this.handlers.onCapture(floatToPcm16(resampled));
    };
    source.connect(worklet);
    worklet.connect(context.destination);
    this.worklet = worklet;
    this.nextTime = context.currentTime;
  }

  play(pcm: ArrayBuffer): void {
    const context = this.context;
    if (!context) return;
    const samples = resample(pcm16ToFloat(pcm), VOICE_SAMPLE_RATE, context.sampleRate);
    if (!samples.length) return;
    const buffer = context.createBuffer(1, samples.length, context.sampleRate);
    buffer.getChannelData(0).set(samples);
    const node = context.createBufferSource();
    node.buffer = buffer;
    node.connect(context.destination);
    const startAt = Math.max(context.currentTime, this.nextTime);
    node.start(startAt);
    this.nextTime = startAt + buffer.duration;
    this.playing.push(node);
    node.onended = () => {
      this.playing = this.playing.filter((item) => item !== node);
    };
  }

  interrupt(): void {
    for (const node of this.playing) {
      try { node.stop(); } catch { /* already stopped */ }
    }
    this.playing = [];
    if (this.context) this.nextTime = this.context.currentTime;
  }

  stop(): void {
    this.stopped = true;
    this.interrupt();
    this.worklet?.disconnect();
    this.worklet = null;
    this.stream?.getTracks().forEach((track) => track.stop());
    this.stream = null;
    void this.context?.close();
    this.context = null;
  }
}
