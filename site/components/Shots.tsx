"use client";

import { DesktopShot, PhoneShot } from "@/components/DesktopShot";
import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";

export type Shot = [name: string, alt: string, caption?: string];
export type Device = "iphone" | "desktop";
export type PhoneLayout = "trio" | "pair" | "fan" | "single";

export function Screen({ name, alt, caption, eager }: { name: string; alt: string; caption?: string; eager?: boolean }) {
  return (
    <figure className="phone">
      <PhoneShot name={name} alt={alt} eager={eager} />
      {caption && <figcaption>{caption}</figcaption>}
    </figure>
  );
}

export function Window({ name, alt, caption, eager }: { name: string; alt: string; caption?: string; eager?: boolean }) {
  return (
    <figure className="window">
      <DesktopShot name={name} alt={alt} eager={eager} />
      {caption && <figcaption>{caption}</figcaption>}
    </figure>
  );
}

// One choice for every switch on the page, kept in memory only.
let current: Device = "iphone";
let chosen = false;
const listeners = new Set<() => void>();
function setDevice(device: Device, byUser: boolean) {
  if (!byUser && chosen) return;
  chosen ||= byUser;
  current = device;
  listeners.forEach((listener) => listener());
}
function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function onComputer() {
  const nav = navigator as Navigator & { userAgentData?: { mobile?: boolean } };
  if (/iphone|ipad|ipod|android|mobile/i.test(nav.userAgent)) return false;
  if (nav.userAgentData) return !nav.userAgentData.mobile;
  return navigator.maxTouchPoints <= 1;
}

/** Picks the visitor's own device once per page, unless they already chose. */
let detected = false;
function detectDevice() {
  if (detected) return;
  detected = true;
  if (onComputer()) setDevice("desktop", false);
}

function useDevice() {
  const device = useSyncExternalStore(subscribe, () => current, () => "iphone" as Device);
  useEffect(detectDevice, []);
  return device;
}

/** Screenshots of the same feature on iPhone and on the desktop, with a switch between them. */
export function DeviceShots({
  labels,
  iphone,
  desktop,
  layout = "trio",
}: {
  labels: { group: string; iphone: string; desktop: string };
  iphone?: Shot[];
  desktop?: Shot[];
  layout?: PhoneLayout;
}) {
  const id = useId();
  const device = useDevice();
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);

  const phones = iphone && (
    <div className={`phones ${layout}`}>
      {iphone.map(([name, alt, caption]) => <Screen key={name} name={name} alt={alt} caption={caption} />)}
    </div>
  );
  const windows = desktop && (
    <div className={`windows count-${Math.min(desktop.length, 2)}`}>
      {desktop.slice(0, 2).map(([name, alt, caption]) => <Window key={name} name={name} alt={alt} caption={caption} />)}
    </div>
  );
  if (!iphone || !desktop) return <div className="device-shots">{phones ?? windows}</div>;

  const options: Device[] = ["iphone", "desktop"];
  const select = (index: number) => {
    setDevice(options[index], true);
    tabs.current[index]?.focus();
  };
  return (
    <div className="device-shots" data-switchable="">
      <div className="device-switch" role="tablist" aria-label={labels.group}>
        {options.map((option, index) => (
          <button
            key={option}
            ref={(element) => { tabs.current[index] = element; }}
            type="button"
            role="tab"
            id={`${id}-${option}-tab`}
            aria-selected={device === option}
            aria-controls={`${id}-panel`}
            tabIndex={device === option ? 0 : -1}
            onClick={() => setDevice(option, true)}
            onKeyDown={(event) => {
              if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
                event.preventDefault();
                select((index + 1) % options.length);
              }
            }}
          >
            {option === "iphone" ? labels.iphone : labels.desktop}
          </button>
        ))}
      </div>
      <div id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-${device}-tab`} className="device-panel">
        {device === "iphone" ? phones : windows}
      </div>
    </div>
  );
}

const ICONS: Record<Device, React.ReactNode> = {
  iphone: <><rect x="7" y="2.5" width="10" height="19" rx="2.6" /><path d="M10.5 5h3" /></>,
  desktop: <><rect x="4" y="4.5" width="16" height="11" rx="1.6" /><path d="M2 19h20" /></>,
};

/**
 * One switch for the whole page on wide screens: a dock on the side that shows while screenshots are on
 * screen, with an indicator that slides to the chosen device. Narrower screens keep a switch per section.
 */
export function DeviceDock({ labels }: { labels: { switch: string; iphone: string; desktop: string } }) {
  const device = useDevice();
  const [shown, setShown] = useState(false);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const options: Device[] = ["iphone", "desktop"];

  useEffect(() => {
    const visible = new Set<Element>();
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) visible.add(entry.target);
        else visible.delete(entry.target);
      }
      setShown(visible.size > 0);
    }, { rootMargin: "-20% 0px -20% 0px" });
    document.querySelectorAll(".device-shots[data-switchable]").forEach((element) => observer.observe(element));
    return () => observer.disconnect();
  }, []);

  return (
    <div className="device-dock" role="radiogroup" aria-label={labels.switch} data-device={device} data-shown={shown || undefined}>
      <span className="device-dock-thumb" aria-hidden="true" />
      {options.map((option, index) => (
        <button
          key={option}
          ref={(element) => { buttons.current[index] = element; }}
          type="button"
          role="radio"
          aria-checked={device === option}
          tabIndex={device === option ? 0 : -1}
          onClick={() => setDevice(option, true)}
          onKeyDown={(event) => {
            if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
            event.preventDefault();
            const next = (index + 1) % options.length;
            setDevice(options[next], true);
            buttons.current[next]?.focus();
          }}
        >
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{ICONS[option]}</svg>
          <span>{option === "iphone" ? labels.iphone : labels.desktop}</span>
        </button>
      ))}
    </div>
  );
}
