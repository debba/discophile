"use client";

import { DesktopShot, PhoneShot } from "@/components/DesktopShot";
import { useEffect, useId, useRef, useSyncExternalStore } from "react";

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
  const device = useSyncExternalStore(subscribe, () => current, () => "iphone" as Device);
  const tabs = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => {
    if (onComputer()) setDevice("desktop", false);
  }, []);

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
    <div className="device-shots">
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
