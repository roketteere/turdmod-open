// Shared Web Audio plumbing. ONE AudioContext for the whole app, but a
// SEPARATE MediaElementSource + AnalyserNode per <audio> element — so the
// intro splash and the lobby each get their own independent analyser feeding
// their own visualizer. createMediaElementSource can only be called once per
// element, so we cache the source per element.

let ctx: AudioContext | null = null;
const sources = new WeakMap<HTMLMediaElement, MediaElementAudioSourceNode>();

export function getCtx(): AudioContext | null {
  if (ctx) return ctx;
  try {
    ctx = new AudioContext();
  } catch {
    ctx = null;
  }
  return ctx;
}

/** Build an AnalyserNode for one media element (source created once + cached).
 *  Returns null if Web Audio is unavailable. Resumes a suspended context. */
export function makeAnalyser(el: HTMLMediaElement, fftSize = 512): AnalyserNode | null {
  const c = getCtx();
  if (!c) return null;
  try {
    let src = sources.get(el);
    if (!src) {
      src = c.createMediaElementSource(el);
      sources.set(el, src);
    }
    const an = c.createAnalyser();
    an.fftSize = fftSize;
    an.smoothingTimeConstant = 0.72;
    src.connect(an);
    an.connect(c.destination); // keep audio audible
    if (c.state === "suspended") c.resume().catch(() => {});
    return an;
  } catch {
    return null;
  }
}

export function resumeCtx() {
  if (ctx && ctx.state === "suspended") ctx.resume().catch(() => {});
}
