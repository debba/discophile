"use client";

import { useEffect, useRef } from "react";

/**
 * The hero record. CSS spins it once on load; it spins again whenever it scrolls back into view,
 * so the motion stays brief but the turntable is playing each time you return to it.
 */
export function SpinningRecord() {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const record = ref.current;
    if (!record || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    let away = false;
    const observer = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting) {
        away = true;
        return;
      }
      if (!away) return;
      away = false;
      record.getAnimations().forEach((animation) => animation.cancel());
      record.animate([{ rotate: "0deg" }, { rotate: "360deg" }], { duration: 4000, easing: "cubic-bezier(0.3, 0, 0.3, 1)" });
    }, { threshold: 0.25 });
    observer.observe(record);
    return () => observer.disconnect();
  }, []);

  return <div ref={ref} className="record"><div className="label" /></div>;
}
