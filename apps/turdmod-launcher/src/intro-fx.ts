// Intro splash lights — beat-reactive but CALM (slower than the lobby rave).
// Reads its OWN analyser (the intro track) so it's independent of the lobby
// visualizer. Soft pulsing radial glows that swell on the beat; if no audio
// data is flowing yet, it falls back to a slow ambient breathe (never the
// old fast fixed strobe).
//
// @dep: App.tsx passes the intro AnalyserNode (or null → ambient-only).

const PALETTE = [
  [0, 212, 255],   // cyan
  [153, 69, 255],  // purple
  [255, 45, 124],  // pink
];

export function attachIntroFx(canvas: HTMLCanvasElement, analyser: AnalyserNode | null) {
  const ctx = canvas.getContext("2d")!;
  const bins = analyser ? new Uint8Array(analyser.frequencyBinCount) : null;
  let raf = 0, W = 0, H = 0, DPR = 1;

  function resize() {
    DPR = Math.min(window.devicePixelRatio || 1, 2);
    W = canvas.clientWidth; H = canvas.clientHeight;
    canvas.width = Math.max(1, Math.floor(W * DPR));
    canvas.height = Math.max(1, Math.floor(H * DPR));
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  }
  resize();
  window.addEventListener("resize", resize);

  let bassAvg = 0, pulse = 0, beatCooldown = 0, hue = 0;

  function frame() {
    let bass = 0, level = 0;
    if (analyser && bins) {
      analyser.getByteFrequencyData(bins);
      let bs = 0; for (let i = 0; i < 6; i++) bs += bins[i];
      bass = bs / (6 * 255);
      let lv = 0; for (let i = 0; i < 60; i++) lv += bins[i];
      level = lv / (60 * 255);
    }
    bassAvg = bassAvg * 0.92 + bass * 0.08;

    // calm beat detection — bigger refractory so it's not frantic
    if (beatCooldown > 0) beatCooldown--;
    if (bass > bassAvg * 1.3 && bass > 0.28 && beatCooldown === 0) {
      beatCooldown = 12; // ~200ms min → relaxed cadence, not strobe-fast
      pulse = Math.min(1, 0.7 + bass);
    }
    pulse *= 0.92; // slow decay

    // ambient floor so it always breathes even before audio flows
    const t = performance.now() / 1000;
    const ambient = 0.12 + 0.06 * Math.sin(t * 0.8);
    const glow = Math.max(ambient, pulse);
    hue = (hue + 0.15 + bass * 1.2) % 360;

    // soft trailing clear (slow → smooth, not flickery)
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = "rgba(8, 8, 15, 0.20)";
    ctx.fillRect(0, 0, W, H);
    ctx.globalCompositeOperation = "lighter";

    const cx = W / 2, cy = H * 0.42;
    // two gentle drifting glows behind the logo
    for (let i = 0; i < 3; i++) {
      const col = PALETTE[(i + Math.floor(hue / 120)) % PALETTE.length];
      const ox = Math.sin(t * (0.25 + i * 0.12) + i * 2) * W * 0.18;
      const oy = Math.cos(t * (0.2 + i * 0.1) + i) * H * 0.10;
      const r = Math.min(W, H) * (0.30 + glow * 0.30 + level * 0.15);
      const g = ctx.createRadialGradient(cx + ox, cy + oy, 0, cx + ox, cy + oy, r);
      const a = 0.05 + glow * 0.18;
      g.addColorStop(0, `rgba(${col[0]},${col[1]},${col[2]},${a})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.beginPath(); ctx.arc(cx + ox, cy + oy, r, 0, Math.PI * 2); ctx.fill();
    }

    raf = requestAnimationFrame(frame);
  }
  raf = requestAnimationFrame(frame);
  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", resize);
  };
}
