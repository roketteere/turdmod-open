// Beat-reactive party visualizer for the lobby. Live Web Audio data drives
// everything, so it tracks ANY song. Canvas sits BEHIND the UI.
//
// Layers: drifting color CLOUDS (nebula) + electric WAVEFORM lines + LIGHTNING
// bolts on beats + beat particles. No vertical bars. Palette = x12 cyan/
// blue/purple/magenta/pink. Also publishes a live `--beat` CSS var on :root
// (0..1) so the PLAY button can pulse with the music.
//
// @dep: App.tsx builds the AnalyserNode on the ENTER gesture and passes it in.

const PALETTE = [
  [0, 212, 255],   // neon cyan
  [80, 140, 255],  // blue
  [153, 69, 255],  // purple
  [200, 60, 200],  // magenta
  [255, 45, 124],  // pink
];

const TOP_SAFE = 84; // px reserved for window chrome + topbar

type Cloud = {
  bx: number; by: number; ox: number; oy: number; sx: number; sy: number;
  band: [number, number]; col: number[]; baseR: number;
};
type Particle = { x: number; y: number; vx: number; vy: number; life: number; hue: number; size: number };
type Bolt = { pts: { x: number; y: number }[]; life: number; col: number[] };

export function attachParty(canvas: HTMLCanvasElement, analyser: AnalyserNode) {
  const ctx = canvas.getContext("2d")!;
  const N = analyser.frequencyBinCount;     // freq bins (256 @ fft 512)
  const freq = new Uint8Array(N);
  const wave = new Uint8Array(analyser.fftSize); // time-domain samples
  let raf = 0;
  let W = 0, H = 0, DPR = 1;

  function resize() {
    DPR = Math.min(window.devicePixelRatio || 1, 2);
    W = canvas.clientWidth; H = canvas.clientHeight;
    canvas.width = Math.max(1, Math.floor(W * DPR));
    canvas.height = Math.max(1, Math.floor(H * DPR));
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  }
  resize();
  window.addEventListener("resize", resize);

  const clouds: Cloud[] = [
    { bx: 0.22, by: 0.45, ox: 0, oy: 1.1, sx: 0.05, sy: 0.07, band: [0, 6], col: PALETTE[4], baseR: 0.42 },
    { bx: 0.72, by: 0.40, ox: 2, oy: 0.4, sx: 0.06, sy: 0.05, band: [6, 16], col: PALETTE[2], baseR: 0.40 },
    { bx: 0.50, by: 0.60, ox: 4, oy: 2.2, sx: 0.04, sy: 0.06, band: [16, 36], col: PALETTE[0], baseR: 0.38 },
    { bx: 0.30, by: 0.30, ox: 1, oy: 3.0, sx: 0.07, sy: 0.04, band: [36, 70], col: PALETTE[1], baseR: 0.32 },
    { bx: 0.82, by: 0.68, ox: 3, oy: 1.7, sx: 0.05, sy: 0.08, band: [70, 130], col: PALETTE[3], baseR: 0.30 },
    { bx: 0.12, by: 0.70, ox: 5, oy: 0.9, sx: 0.06, sy: 0.05, band: [4, 12], col: PALETTE[0], baseR: 0.34 },
  ];

  let smoothEnergy = 0, bassAvg = 0, hueShift = 0, flash = 0, beatCooldown = 0;
  let beatPulse = 0; // decays each frame; published to --beat
  const particles: Particle[] = [];
  const bolts: Bolt[] = [];

  function band(lo: number, hi: number) {
    let s = 0; const top = Math.min(hi, N);
    for (let i = lo; i < top; i++) s += freq[i];
    return s / (Math.max(1, top - lo) * 255);
  }

  // jagged lightning bolt between two points (recursive midpoint displacement)
  function makeBolt(x1: number, y1: number, x2: number, y2: number, disp: number): { x: number; y: number }[] {
    const pts = [{ x: x1, y: y1 }, { x: x2, y: y2 }];
    for (let depth = 0; depth < 5; depth++) {
      const next: { x: number; y: number }[] = [];
      for (let i = 0; i < pts.length - 1; i++) {
        const a = pts[i], b = pts[i + 1];
        const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2;
        // perpendicular offset
        const nx = -(b.y - a.y), ny = b.x - a.x;
        const len = Math.hypot(nx, ny) || 1;
        const off = (Math.random() - 0.5) * disp;
        next.push(a, { x: mx + (nx / len) * off, y: my + (ny / len) * off });
      }
      next.push(pts[pts.length - 1]);
      pts.length = 0; pts.push(...next);
      disp *= 0.55;
    }
    return pts;
  }

  function frame() {
    analyser.getByteFrequencyData(freq);
    analyser.getByteTimeDomainData(wave);

    const bass = band(0, 6);
    const mid = band(8, 40);
    const treble = band(40, 110);
    const level = band(0, 140);
    smoothEnergy = smoothEnergy * 0.9 + level * 0.1;
    bassAvg = bassAvg * 0.92 + bass * 0.08;
    const speed = 0.25 + smoothEnergy * 1.6;
    hueShift = (hueShift + 0.2 + bass * 2.2) % 360;

    // beat detection
    let isBeat = false;
    if (beatCooldown > 0) beatCooldown--;
    if (bass > bassAvg * 1.32 && bass > 0.30 && beatCooldown === 0) {
      isBeat = true; beatCooldown = 6;
      flash = Math.min(1, flash + 0.4 + bass * 0.5);
      beatPulse = Math.min(1, 0.6 + bass);
      // particles
      const cx = W / 2, cy = H * 0.6;
      const count = 8 + Math.floor(bass * 22);
      for (let i = 0; i < count; i++) {
        const a = Math.random() * Math.PI * 2;
        const sp = 2 + Math.random() * (3 + bass * 8);
        particles.push({ x: cx, y: cy, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp, life: 1, hue: Math.random() * 360, size: 2 + Math.random() * 3 });
      }
      // lightning: a couple of bolts arcing across the body on bigger beats
      const boltN = bass > 0.5 ? 3 : 1;
      for (let i = 0; i < boltN; i++) {
        const y1 = TOP_SAFE + Math.random() * (H - TOP_SAFE);
        const y2 = TOP_SAFE + Math.random() * (H - TOP_SAFE);
        bolts.push({ pts: makeBolt(0, y1, W, y2, 220), life: 1, col: PALETTE[Math.floor(Math.random() * PALETTE.length)] });
      }
    }
    flash *= 0.88;
    beatPulse *= 0.85;
    // publish beat to CSS so the PLAY button can dance
    document.documentElement.style.setProperty("--beat", beatPulse.toFixed(3));

    // trailing fade
    ctx.globalCompositeOperation = "source-over";
    ctx.fillStyle = "rgba(8, 8, 15, 0.32)";
    ctx.fillRect(0, 0, W, H);
    ctx.globalCompositeOperation = "lighter";

    // ---- color clouds ----
    const t = (performance.now() / 1000) * speed;
    const minDim = Math.min(W, H);
    const bodyBottom = H - 40;
    for (const c of clouds) {
      const e = band(c.band[0], c.band[1]);
      const x = (c.bx + Math.sin(t * c.sx + c.ox) * 0.10) * W;
      const y = TOP_SAFE + c.by * (bodyBottom - TOP_SAFE) + Math.sin(t * c.sy + c.oy) * 0.06 * H;
      const r = minDim * c.baseR * (0.55 + e * 0.95 + flash * 0.15);
      const mix = Math.floor(hueShift / 72) % PALETTE.length;
      const col = [
        (c.col[0] + PALETTE[mix][0]) >> 1,
        (c.col[1] + PALETTE[mix][1]) >> 1,
        (c.col[2] + PALETTE[mix][2]) >> 1,
      ];
      const alpha = 0.06 + e * 0.28;
      const g = ctx.createRadialGradient(x, y, 0, x, y, r);
      g.addColorStop(0, `rgba(${col[0]},${col[1]},${col[2]},${alpha})`);
      g.addColorStop(0.5, `rgba(${col[0]},${col[1]},${col[2]},${alpha * 0.35})`);
      g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.fillStyle = g;
      ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill();
    }

    // ---- electric WAVEFORM lines (oscilloscope) ----
    // Three stacked, color-cycled waves reading the live time-domain signal.
    // Amplitude scales with mid/treble energy → the wave "electrifies" on hits.
    const baseYs = [H * 0.5, H * 0.66, H * 0.8];
    const amps = [
      H * (0.10 + treble * 0.20),
      H * (0.08 + mid * 0.18),
      H * (0.06 + bass * 0.16),
    ];
    for (let w = 0; w < 3; w++) {
      const col = PALETTE[(w + Math.floor(hueShift / 72)) % PALETTE.length];
      ctx.lineWidth = 2 + (w === 0 ? treble * 3 : mid * 2);
      ctx.strokeStyle = `rgba(${col[0]},${col[1]},${col[2]},${0.35 + level * 0.5})`;
      ctx.shadowColor = `rgba(${col[0]},${col[1]},${col[2]},0.9)`;
      ctx.shadowBlur = 12 + level * 24; // neon glow
      ctx.beginPath();
      const step = 2;
      for (let x = 0; x <= W; x += step) {
        const idx = Math.floor((x / W) * wave.length);
        const s = (wave[idx] - 128) / 128; // -1..1
        const y = baseYs[w] + s * amps[w] * (0.6 + 0.6 * Math.sin(t + w));
        if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
      }
      ctx.stroke();
    }
    ctx.shadowBlur = 0;

    // ---- lightning bolts ----
    for (let i = bolts.length - 1; i >= 0; i--) {
      const b = bolts[i];
      b.life -= 0.08;
      if (b.life <= 0) { bolts.splice(i, 1); continue; }
      ctx.strokeStyle = `rgba(${b.col[0]},${b.col[1]},${b.col[2]},${b.life})`;
      ctx.shadowColor = `rgba(${b.col[0]},${b.col[1]},${b.col[2]},1)`;
      ctx.shadowBlur = 18;
      ctx.lineWidth = 1.5 + b.life * 2.5;
      ctx.beginPath();
      ctx.moveTo(b.pts[0].x, b.pts[0].y);
      for (let k = 1; k < b.pts.length; k++) ctx.lineTo(b.pts[k].x, b.pts[k].y);
      ctx.stroke();
    }
    ctx.shadowBlur = 0;

    // ---- particles ----
    for (let i = particles.length - 1; i >= 0; i--) {
      const p = particles[i];
      p.x += p.vx; p.y += p.vy; p.vy += 0.05; p.vx *= 0.985; p.life -= 0.02;
      if (p.life <= 0) { particles.splice(i, 1); continue; }
      const hue = (200 + (p.hue % 120) + hueShift) % 360;
      ctx.fillStyle = `hsla(${hue}, 100%, 66%, ${p.life})`;
      ctx.beginPath(); ctx.arc(p.x, p.y, p.size * p.life, 0, Math.PI * 2); ctx.fill();
    }
    if (particles.length > 500) particles.splice(0, particles.length - 500);

    // ---- soft beat bloom ----
    if (flash > 0.02) {
      const fc = PALETTE[Math.floor(hueShift / 72) % PALETTE.length];
      ctx.fillStyle = `rgba(${fc[0]},${fc[1]},${fc[2]},${flash * 0.10})`;
      ctx.fillRect(0, TOP_SAFE, W, H - TOP_SAFE);
    }

    void isBeat;
    raf = requestAnimationFrame(frame);
  }

  raf = requestAnimationFrame(frame);
  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", resize);
    document.documentElement.style.setProperty("--beat", "0");
  };
}
