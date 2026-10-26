"use client";

import { createContext, useCallback, useContext, useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { trackEvent } from "@/lib/analytics";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { RELEASES_URL, megabytes, type DownloadFormat, type Platform, type Release } from "@/lib/releases";
import { TestFlightButton } from "@/components/TestFlight";

const PLATFORMS: Platform[] = ["mac", "windows", "linux"];
const AUR_COMMAND = "yay -S discophile-bin";

type Detected = Platform | "ios" | null;

function detect(): Detected {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const agent = nav.userAgent.toLowerCase();
  if (/iphone|ipad|ipod/.test(agent)) return "ios";
  if (agent.includes("android")) return null;
  const name = (nav.userAgentData?.platform || agent).toLowerCase();
  // iPadOS reports itself as a Mac; a touch screen gives it away.
  if (name.includes("mac")) return navigator.maxTouchPoints > 1 ? "ios" : "mac";
  if (name.includes("win")) return "windows";
  if (name.includes("linux") || name.includes("x11")) return "linux";
  return null;
}

const DownloadContext = createContext<{ open: (platform?: Platform) => void; detected: Detected; locale: Locale }>({
  open: () => {},
  detected: null,
  locale: "en",
});

export function DownloadProvider({ locale, release, children }: { locale: Locale; release: Release | null; children: ReactNode }) {
  const t = ui[locale].download;
  const dialogRef = useRef<HTMLDialogElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const tabsRef = useRef<(HTMLButtonElement | null)[]>([]);
  const [detected, setDetected] = useState<Detected>(null);
  const [platform, setPlatform] = useState<Platform>("mac");
  const [copied, setCopied] = useState(false);
  // The file being downloaded: the sheet then thanks the visitor and offers sponsorship and the newsletter.
  const [started, setStarted] = useState<{ url: string; name: string } | null>(null);
  const thanksRef = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    const found = detect();
    setDetected(found);
    if (found && found !== "ios") setPlatform(found);
  }, []);

  const open = useCallback((choice?: Platform) => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    if (choice) setPlatform(choice);
    openerRef.current = document.activeElement as HTMLElement | null;
    dialog.showModal();
    trackEvent("Download", "Open", choice ?? "default");
    requestAnimationFrame(() => dialog.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus());
  }, []);

  const close = useCallback(() => dialogRef.current?.close(), []);

  const select = (index: number) => {
    setPlatform(PLATFORMS[index]);
    setCopied(false);
    tabsRef.current[index]?.focus();
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(AUR_COMMAND);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // The command stays visible and selectable.
    }
  };

  const formats: DownloadFormat[] = platform === "linux"
    ? (["deb", "appimage", "linux"] as const).filter((format) => !!release?.files[format])
    : [platform];
  if (formats.length === 0) formats.push(platform);

  return (
    <DownloadContext.Provider value={{ open, detected, locale }}>
      {children}
      <dialog
        ref={dialogRef}
        className="sheet download-sheet"
        aria-labelledby="download-title"
        onClose={() => {
          setCopied(false);
          setStarted(null);
          openerRef.current?.focus();
        }}
      >
        <div className="sheet-head">
          <div className="sheet-title">
            <img src="/assets/icon-72.png" alt="" width={44} height={44} loading="lazy" />
            <h2 id="download-title">{started ? ui[locale].newsletter.started : t.title}</h2>
          </div>
          <button type="button" className="dialog-close" onClick={close}>
            <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" /></svg>
            <span className="sr-only">{t.close}</span>
          </button>
        </div>

        {started ? (
          <Thanks locale={locale} file={started} headingRef={thanksRef} onBack={() => setStarted(null)} />
        ) : (
        <>
        <div className="sheet-body">
          <div className="device-switch platform-switch" role="tablist" aria-label={t.platforms}>
            {PLATFORMS.map((option, index) => (
              <button
                key={option}
                ref={(element) => { tabsRef.current[index] = element; }}
                type="button"
                role="tab"
                id={`download-tab-${option}`}
                aria-selected={platform === option}
                aria-controls="download-panel"
                tabIndex={platform === option ? 0 : -1}
                onClick={() => select(index)}
                onKeyDown={(event) => {
                  const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
                  if (step) {
                    event.preventDefault();
                    select((index + step + PLATFORMS.length) % PLATFORMS.length);
                  }
                }}
              >
                {t.names[option]}
              </button>
            ))}
          </div>

          <div className="download-meta">
            {release && <span className="version-tag">{t.version} {release.version}</span>}
            <a href={release?.page ?? RELEASES_URL} target="_blank" rel="noopener noreferrer">
              {t.notes}<span aria-hidden="true"> ↗</span><span className="sr-only"> {t.newTab}</span>
            </a>
          </div>

          <div id="download-panel" role="tabpanel" aria-labelledby={`download-tab-${platform}`}>
            <ul className="download-options">
              {formats.map((format) => {
                const file = release?.files[format];
                const [label, detail] = format === "deb" || format === "appimage"
                  ? t.linuxPackages[format]
                  : t.options[format][0];
                return (
                  <li key={format}>
                    <div>
                      <strong>{label}</strong>
                      <span>{detail.replace("VERSION", release?.version ?? "VERSION")}{file && <> · <span className="nowrap">{megabytes(file.size)}</span></>}</span>
                    </div>
                    {file ? (
                      <a
                        className="button primary"
                        href={file.url}
                        onClick={() => {
                          trackEvent("Download", "File", format);
                          setStarted({ url: file.url, name: file.name });
                          requestAnimationFrame(() => thanksRef.current?.focus());
                        }}
                        aria-label={`${t.file} ${file.name}`}
                      >
                        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14" /></svg>
                        {file.name.endsWith(".tar.gz") ? ".tar.gz" : `.${file.name.split(".").pop()}`}
                      </a>
                    ) : (
                      <a className="button" href={release?.page ?? RELEASES_URL}>GitHub</a>
                    )}
                  </li>
                );
              })}
              {platform === "linux" && (
                <li className="download-command">
                  <div>
                    <strong>{t.aur[0]}</strong>
                    <span>{t.aur[1]}</span>
                  </div>
                  <div className="command-row">
                    <code>{AUR_COMMAND}</code>
                    <button type="button" className="copy-button" onClick={copy}>
                      {copied ? t.copied : t.copy}
                    </button>
                  </div>
                </li>
              )}
            </ul>
            <p className="download-tip">{formats.some((format) => release?.files[format]) ? t.tips[platform] : t.unavailable}</p>
            <p className="sr-only" role="status">{copied ? t.copied : ""}</p>
          </div>
        </div>

        <div className="sheet-foot">
          <span>
            {t.iphone}{" "}
            <TestFlightButton className="link-button" onBeforeOpen={close}>{t.iphoneLink}</TestFlightButton>
          </span>
          <span>
            {t.web} <a href={localePath(locale, "/self-hosting")}>{t.webLink}</a>
          </span>
        </div>
        </>
        )}
      </dialog>
    </DownloadContext.Provider>
  );
}

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** After a download starts: a direct link, GitHub Sponsors and the newsletter signup. */
function Thanks({
  locale,
  file,
  headingRef,
  onBack,
}: {
  locale: Locale;
  file: { url: string; name: string };
  headingRef: React.RefObject<HTMLHeadingElement | null>;
  onBack: () => void;
}) {
  const t = ui[locale].newsletter;
  const id = useId();
  const [status, setStatus] = useState<"idle" | "sending" | "done">("idle");
  const [error, setError] = useState<{ field?: "email" | "consent"; message: string } | null>(null);
  const emailRef = useRef<HTMLInputElement>(null);
  const consentRef = useRef<HTMLInputElement>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (status === "sending") return;
    const data = new FormData(event.currentTarget);
    const email = String(data.get("email") ?? "").trim();
    const consent = data.get("consent") === "yes";
    const local = !EMAIL_PATTERN.test(email)
      ? { field: "email" as const, message: t.invalid }
      : !consent ? { field: "consent" as const, message: t.consentRequired } : null;
    if (local) {
      setError(local);
      (local.field === "email" ? emailRef : consentRef).current?.focus();
      return;
    }
    setStatus("sending");
    setError(null);
    try {
      const response = await fetch("/api/newsletter", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, consent, locale, website: data.get("website") }),
      });
      const result = await response.json().catch(() => ({}));
      if (!response.ok) {
        setStatus("idle");
        setError({ field: result.field, message: result.error ?? t.failed });
        if (result.field === "email") emailRef.current?.focus();
        return;
      }
      setStatus("done");
      trackEvent("Newsletter", "Subscribe");
    } catch {
      setStatus("idle");
      setError({ message: t.failed });
    }
  }

  return (
    <div className="sheet-body thanks">
      <p className="thanks-lead">
        <span ref={headingRef} tabIndex={-1}>{t.startedBody}</span>{" "}
        <a href={file.url}>{t.directLink}</a>.
      </p>

      <section className="thanks-card sponsor-card" aria-labelledby={`${id}-sponsor`}>
        <h3 id={`${id}-sponsor`}>{t.sponsorTitle}</h3>
        <p>{t.sponsorBody}</p>
        <a className="button" href="https://github.com/sponsors/debba" target="_blank" rel="noopener noreferrer" onClick={() => trackEvent("Sponsor", "Download sheet")}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z" /></svg>
          {t.sponsorButton}<span className="sr-only"> {ui[locale].download.newTab}</span>
        </a>
      </section>

      <section className="thanks-card" aria-labelledby={`${id}-news`}>
        <h3 id={`${id}-news`}>{t.title}</h3>
        <p>{t.body}</p>
        {status === "done" ? (
          <p className="newsletter-done" role="status">{t.done}</p>
        ) : (
          <form className="newsletter" onSubmit={submit} noValidate>
            <label htmlFor={`${id}-email`}>{t.email} <span className="required" aria-hidden="true">*</span></label>
            <div className="newsletter-row">
              <input
                ref={emailRef} id={`${id}-email`} name="email" type="email" autoComplete="email" inputMode="email"
                spellCheck={false} required maxLength={254}
                aria-invalid={error?.field === "email" ? true : undefined}
                aria-describedby={error?.field === "email" ? `${id}-error` : undefined}
              />
              <button type="submit" className="button primary" aria-disabled={status === "sending"}>
                {status === "sending" ? t.sending : t.submit}
              </button>
            </div>
            <label className="choice consent">
              <input
                ref={consentRef} type="checkbox" name="consent" value="yes" required
                aria-invalid={error?.field === "consent" ? true : undefined}
                aria-describedby={error?.field === "consent" ? `${id}-error` : undefined}
              />
              <span>
                {t.consentBefore}{locale === "en" ? " " : ""}
                <a href={`${localePath(locale, "/privacy")}#newsletter`} target="_blank" rel="noopener">{t.consentLink}<span className="sr-only"> {ui[locale].download.newTab}</span></a>.{" "}
                <span className="required" aria-hidden="true">*</span>
              </span>
            </label>
            <div className="honeypot" aria-hidden="true">
              <label htmlFor={`${id}-website`}>Website</label>
              <input id={`${id}-website`} name="website" type="text" tabIndex={-1} autoComplete="off" />
            </div>
            {error && <p className="field-error" id={`${id}-error`} role="alert">{error.message}</p>}
            <p className="sr-only" role="status">{status === "sending" ? t.sending : ""}</p>
          </form>
        )}
      </section>

      <button type="button" className="link-button thanks-back" onClick={onBack}>← {t.back}</button>
    </div>
  );
}

const DOWNLOAD_ICON = (
  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14" />
  </svg>
);

/**
 * Opens the download sheet for the visitor's computer. On iPhone and iPad it shows `onIOS` instead,
 * or nothing with `hideOnIOS`.
 */
export function DownloadButton({
  labels,
  onIOS,
  hideOnIOS,
  platform,
  className = "button primary",
}: {
  labels: { any: string; mac: string; windows: string; linux: string };
  onIOS?: ReactNode;
  hideOnIOS?: boolean;
  platform?: Platform;
  className?: string;
}) {
  const { open, detected } = useContext(DownloadContext);
  if (!platform && detected === "ios") return hideOnIOS ? null : <>{onIOS}</>;
  const target = platform ?? (detected === "ios" ? null : detected);
  return (
    <button type="button" className={className} aria-haspopup="dialog" onClick={() => open(target ?? undefined)}>
      {DOWNLOAD_ICON}
      {target ? labels[target] : labels.any}
    </button>
  );
}
