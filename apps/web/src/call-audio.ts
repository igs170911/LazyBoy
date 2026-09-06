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

// One frame per 128-sample worklet callback would be ~375 websocket messages a
// second; batching to 40 ms keeps latency low without flooding the provider.
export const CAPTURE_CHUNK_SAMPLES = VOICE_SAMPLE_RATE / 25;
// Cushion so a late chunk does not have to start in the past and click.
const PLAYBACK_LEAD_SECONDS = 0.08;

export type CallAudioHandlers = {
  onCapture: (pcm: Uint8Array) => void;
  onError: (message: string) => void;
  onPlaybackChange?: (playing: boolean) => void;
};

export class CallAudio {
  private stopped = false;
  private context: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private worklet: AudioWorkletNode | null = null;
  private monitor: GainNode | null = null;
  private nextTime = 0;
  private playing: AudioBufferSourceNode[] = [];
  private playbackActive = false;
  private pending: Float32Array[] = [];
  private pendingLength = 0;
  private carry: Uint8Array | null = null;
  private handlers: CallAudioHandlers;

  constructor(handlers: CallAudioHandlers) {
    this.handlers = handlers;
  }

  private queueCapture(samples: Float32Array): void {
    if (samples.length) {
      this.pending.push(samples);
      this.pendingLength += samples.length;
    }
    while (this.pendingLength >= CAPTURE_CHUNK_SAMPLES) {
      const chunk = new Float32Array(CAPTURE_CHUNK_SAMPLES);
      let filled = 0;
      while (filled < CAPTURE_CHUNK_SAMPLES) {
        const head = this.pending[0] as Float32Array;
        const take = Math.min(head.length, CAPTURE_CHUNK_SAMPLES - filled);
        chunk.set(head.subarray(0, take), filled);
        filled += take;
        if (take === head.length) this.pending.shift();
        else this.pending[0] = head.subarray(take);
      }
      this.pendingLength -= CAPTURE_CHUNK_SAMPLES;
      this.handlers.onCapture(floatToPcm16(chunk));
    }
  }

  private notifyPlayback(): void {
    const active = this.playing.length > 0;
    if (active === this.playbackActive) return;
    this.playbackActive = active;
    this.handlers.onPlaybackChange?.(active);
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
      this.queueCapture(resample(samples, context.sampleRate, VOICE_SAMPLE_RATE));
    };
    worklet.onprocessorerror = () => this.handlers.onError("capture stopped");
    // Muted sink: the processor emits no output, and a zero gain guarantees the
    // microphone can never leak back into the speakers.
    const monitor = context.createGain();
    monitor.gain.value = 0;
    source.connect(worklet);
    worklet.connect(monitor);
    monitor.connect(context.destination);
    this.worklet = worklet;
    this.monitor = monitor;
    this.nextTime = context.currentTime;
  }

  play(pcm: ArrayBuffer): void {
    const context = this.context;
    if (!context) return;
    // PCM16 frames can be split mid-sample across websocket messages; dropping
    // the odd tail byte would shift every later sample and turn speech to noise.
    let bytes = new Uint8Array(pcm);
    if (this.carry) {
      const joined = new Uint8Array(this.carry.length + bytes.length);
      joined.set(this.carry, 0);
      joined.set(bytes, this.carry.length);
      bytes = joined;
      this.carry = null;
    }
    if (bytes.length % 2 === 1) {
      this.carry = bytes.slice(bytes.length - 1);
      bytes = bytes.subarray(0, bytes.length - 1);
    }
    if (!bytes.length) return;
    const aligned = bytes.slice().buffer;
    const samples = resample(pcm16ToFloat(aligned), VOICE_SAMPLE_RATE, context.sampleRate);
    if (!samples.length) return;
    try {
      const buffer = context.createBuffer(1, samples.length, context.sampleRate);
      buffer.getChannelData(0).set(samples);
      const node = context.createBufferSource();
      node.buffer = buffer;
      node.connect(context.destination);
      if (this.nextTime < context.currentTime) this.nextTime = context.currentTime + PLAYBACK_LEAD_SECONDS;
      const startAt = this.nextTime;
      node.start(startAt);
      this.nextTime = startAt + buffer.duration;
      this.playing.push(node);
      node.onended = () => {
        this.playing = this.playing.filter((item) => item !== node);
        this.notifyPlayback();
      };
      this.notifyPlayback();
    } catch {
      this.handlers.onError("playback failed");
    }
  }

  interrupt(): void {
    for (const node of this.playing) {
      node.onended = null;
      try { node.stop(); } catch { /* already stopped */ }
    }
    this.playing = [];
    this.carry = null;
    if (this.context) this.nextTime = this.context.currentTime;
    this.notifyPlayback();
  }

  stop(): void {
    this.stopped = true;
    this.interrupt();
    this.pending = [];
    this.pendingLength = 0;
    this.worklet?.disconnect();
    this.worklet = null;
    this.monitor?.disconnect();
    this.monitor = null;
    this.stream?.getTracks().forEach((track) => track.stop());
    this.stream = null;
    void this.context?.close();
    this.context = null;
  }
}
