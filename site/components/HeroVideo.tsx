"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { trackEvent } from "@/lib/analytics";
import { localePath, ui, type Locale } from "@/lib/i18n";

const VIDEO = "/assets/video/discophile-intro.mp4?v=2";

/** A real route link, with client-side navigation inside the current locale. */
export function HeroVideo({ locale }: { locale: Locale }) {
  const t = ui[locale].video;
  return (
    <Link href={localePath(locale, "/video")} className="hype-sticker">
      <span className="hype-play" aria-hidden="true">
        <svg width="14" height="14" viewBox="0 0 24 24"><path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.6-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5Z" fill="currentColor" /></svg>
      </span>
      <span className="hype-label">{t.watch}</span>
      <span className="hype-time" aria-hidden="true">0:58</span>
      <span className="sr-only">, {t.duration}</span>
    </Link>
  );
}

export function VideoContent({ locale, standalone = false, stageRef, videoRef, onPlay }: {
  locale: Locale;
  standalone?: boolean;
  stageRef?: React.Ref<HTMLDivElement>;
  videoRef?: React.Ref<HTMLVideoElement>;
  onPlay?: () => void;
}) {
  const t = ui[locale].video;
  const Heading = standalone ? "h1" : "h2";
  const [captions, setCaptions] = useState(false);
  return (
        <div className="film-inner">
          <div ref={stageRef} className="film-stage">
            <video
              ref={videoRef}
              src={VIDEO}
              controls
              playsInline
              autoPlay={standalone}
              preload={standalone ? "auto" : "none"}
              poster="/assets/video/discophile-intro-poster.webp"
              width={1920}
              height={1080}
              onLoadedMetadata={() => setCaptions(true)}
              onPlaying={onPlay}
              aria-label={t.region}
            >
              {captions && <track kind="captions" src="/assets/video/discophile-intro.en.vtt" srcLang="en" label={t.captions} />}
              <a href={VIDEO}>{t.download}</a>
            </video>
          </div>
          <div className="film-meta">
            <Heading id="film-title">{t.title}</Heading>
            <p id="film-note">{t.note}</p>
            <p className="film-links"><Link href={localePath(locale, standalone ? "/" : "/video")}>{standalone ? t.home : t.permalink}</Link> · <a href={VIDEO} download>{t.download}</a></p>
            <details className="transcript">
            <summary>{t.transcript}</summary>
            <h3>{t.shows}</h3>
            <ol>
              {t.scenes.map((scene) => <li key={scene}>{scene}</li>)}
            </ol>
            <h3>{t.lyrics}</h3>
            <p lang="en">
              Pick a record, any record you like. Drop the needle, here we go, press play.
              Discophile, discophile, uh, uh-huh, uh-huh.
              B-side, B-side, stacked up to the ceiling, but I wanna play them all tonight.
              Deep cuts, deep cuts, every single one, yeah. Put the needle where it feels so right.
              Play it louder, one more time now, baby. Every record&apos;s got a little light.
              Skip the radio, I know what I like, so turn it up and tell me I&apos;m a discophile.
            </p>
          </details>
          </div>
        </div>
  );
}

export function FullscreenVideoPage({ locale }: { locale: Locale }) {
  const t = ui[locale].video;
  const home = localePath(locale, "/");
  const router = useRouter();
  const videoRef = useRef<HTMLVideoElement>(null);
  const tracked = useRef(false);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") router.push(home);
    };
    videoRef.current?.focus({ preventScroll: true });
    videoRef.current?.play().catch(() => {
      // Browsers may require a fresh gesture on a direct external visit.
      // Native controls remain available if autoplay with sound is blocked.
    });
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [home, router]);

  return (
    <main id="main" tabIndex={-1} className="film video-page">
      <div className="film-grooves" aria-hidden="true">
        <div className="film-marquee">
          <span>Discophile • Discophile • Discophile • Discophile • </span>
          <span>Discophile • Discophile • Discophile • Discophile • </span>
        </div>
      </div>
      <VideoContent locale={locale} standalone videoRef={videoRef} onPlay={() => {
        if (tracked.current) return;
        tracked.current = true;
        trackEvent("Video", "Play", "Intro");
      }} />
      <Link className="film-close" href={home}>
        <svg width="22" height="22" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" /></svg>
        <span className="sr-only">{t.close}</span>
      </Link>
    </main>
  );
}
