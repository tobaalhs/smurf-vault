// Sonidos de la interfaz, sintetizados al momento (sin archivos): un tick corto al recorrer el
// carrusel, un pop al elegir una cuenta y un acorde al apretar Jugar. Volumen bajo a propósito.
const Sounds = (() => {
  const VOLUME = 0.5;
  let ctx = null;
  let lastTick = 0;
  let enabled = true;
  try {
    enabled = localStorage.getItem('sound') !== 'off';
  } catch {}

  // El navegador solo deja sonar después de un clic o una tecla.
  function unlock() {
    if (!ctx) ctx = new AudioContext();
    if (ctx.state === 'suspended') ctx.resume();
  }
  addEventListener('pointerdown', unlock, { capture: true });
  addEventListener('keydown', unlock, { capture: true });

  const ready = () => enabled && ctx && ctx.state === 'running';

  function blip({ type = 'sine', from, to, dur, gain, delay = 0 }) {
    const t0 = ctx.currentTime + delay;
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    o.type = type;
    o.frequency.setValueAtTime(from, t0);
    o.frequency.exponentialRampToValueAtTime(to, t0 + dur * 0.6);
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(gain * VOLUME, t0 + 0.004);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
    o.connect(g).connect(ctx.destination);
    o.start(t0);
    o.stop(t0 + dur + 0.02);
  }

  return {
    get enabled() {
      return enabled;
    },
    setEnabled(on) {
      enabled = on;
      try {
        localStorage.setItem('sound', on ? 'on' : 'off');
      } catch {}
    },
    /** Clic seco al pasar cada cuenta; no más de uno cada 30 ms aunque se recorra muy rápido. */
    tick() {
      if (!ready() || ctx.currentTime - lastTick < 0.03) return;
      lastTick = ctx.currentTime;
      blip({ type: 'triangle', from: 2300 + Math.random() * 250, to: 1300, dur: 0.035, gain: 0.05 });
    },
    /** Pop tipo burbuja: tono que sube rápido. */
    pop() {
      if (!ready()) return;
      blip({ from: 300, to: 760, dur: 0.13, gain: 0.16 });
      blip({ type: 'triangle', from: 1800, to: 2400, dur: 0.03, gain: 0.03 });
    },
    play() {
      if (!ready()) return;
      blip({ from: 523, to: 540, dur: 0.16, gain: 0.12 });
      blip({ from: 784, to: 800, dur: 0.24, gain: 0.12, delay: 0.08 });
      blip({ from: 1046, to: 1060, dur: 0.34, gain: 0.08, delay: 0.16 });
    },
  };
})();
