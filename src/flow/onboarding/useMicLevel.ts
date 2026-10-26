import { useEffect, useState } from "react";
import { isFlowPreview } from "../dev/preview";

/** Input level 0..1 from the webview's own microphone stream; null if unavailable. */
export const useMicLevel = (enabled: boolean, deviceName: string | null) => {
  const [level, setLevel] = useState<number | null>(isFlowPreview ? 0.55 : null);

  useEffect(() => {
    if (!enabled || isFlowPreview) return;
    let cancelled = false;
    let stream: MediaStream | null = null;
    let ctx: AudioContext | null = null;
    let raf = 0;

    const start = async () => {
      try {
        const devices = await navigator.mediaDevices.enumerateDevices();
        const match = devices.find(
          (d) => d.kind === "audioinput" && deviceName && d.label === deviceName,
        );
        stream = await navigator.mediaDevices.getUserMedia({
          audio: match ? { deviceId: { exact: match.deviceId } } : true,
        });
        if (cancelled) return;
        ctx = new AudioContext();
        const analyser = ctx.createAnalyser();
        analyser.fftSize = 512;
        ctx.createMediaStreamSource(stream).connect(analyser);
        const buf = new Float32Array(analyser.fftSize);
        let smooth = 0;
        const tick = () => {
          analyser.getFloatTimeDomainData(buf);
          let sum = 0;
          for (const v of buf) sum += v * v;
          const rms = Math.sqrt(sum / buf.length);
          smooth = Math.max(Math.min(1, rms * 6), smooth * 0.85);
          setLevel(smooth);
          raf = requestAnimationFrame(tick);
        };
        tick();
      } catch {
        if (!cancelled) setLevel(null);
      }
    };
    start();

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      stream?.getTracks().forEach((t) => t.stop());
      ctx?.close().catch(() => {});
    };
  }, [enabled, deviceName]);

  return level;
};
