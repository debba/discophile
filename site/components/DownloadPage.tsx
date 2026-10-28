import { ConnectOverview } from "@/components/ConnectOverview";
import { DesktopShot, WebShot } from "@/components/DesktopShot";
import type { ReactNode } from "react";
import { DownloadButton } from "@/components/Download";
import { MobileDownload } from "@/components/MobileDownload";
import { Screen, Window } from "@/components/Shots";
import { SiteFooter } from "@/components/SiteFooter";
import { TestFlightButton } from "@/components/TestFlight";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { AUR_URL, RELEASES_URL, REPO_URL, latestRelease, megabytes, type DesktopPlatform } from "@/lib/releases";

type Download = { title: string; needs: string; body: ReactNode };
type Row = [string, string, string, string];

type Copy = {
  title: string;
  lede: (version: string | null) => string;
  iphone: { title: string; needs: string; body: string; facts: ReactNode[]; soon: string; alt: string };
  android: { title: string; needs: string; body: string };
  desktop: { title: string; body: string; facts: ReactNode[]; alt: string };
  downloads: Record<DesktopPlatform, Download> & { aur: ReactNode; button: Record<DesktopPlatform, string>; fallback: string };
  web: { title: string; needs: string; body: string; facts: string[]; link: string; alt: string };
  compare: { title: string; caption: string; head: [string, string, string, string]; rows: Row[] };
  footer: ReactNode;
};

const COPY: Record<Locale, Copy> = {
  en: {
    title: "Wherever you listen.",
    lede: (version) => `On iPhone, Android, Mac, Windows, Linux, or in any browser from a server you host.${version ? ` Latest release: ${version}.` : ""}`,
    iphone: {
      title: "iPhone",
      needs: "iOS 17 or later",
      body: "A native SwiftUI app that feels at home on iOS, with your music, audiobooks and radio a thumb away.",
      facts: [
        <><b>Apple Intelligence</b> runs the music assistant on the iPhone, with nothing leaving the device.</>,
        <><b>Siri, the Lock Screen and AirPlay</b> control playback, and dictation turns your voice into a request.</>,
        <><b>Sport mode</b> keeps presets for running, walking, cycling and the gym, started from Shortcuts.</>,
      ],
      soon: "Awaiting App Store approval. Until then, use the sideload IPA when available below, or request a TestFlight invitation.",
      alt: "Discophile on iPhone, playing Discophile by Mina Sol",
    },
    android: {
      title: "Android",
      needs: "Android 8 or later",
      body: "Your music server on your Android phone. Install the signed APK directly, without Google Play, and keep the same release version as the other Discophile players.",
    },
    desktop: {
      title: "Mac, Windows and Linux",
      body: "The same Discophile in a window made for a keyboard and a big screen: your library in the sidebar, the player always in reach, and the queue, lyrics, About and the assistant beside what you're browsing.",
      facts: [
        <><b>System media controls</b>: Now Playing on macOS, the media overlay on Windows and MPRIS on Linux, plus a tray menu to keep listening with the window closed.</>,
        <><b>A full menu bar on the Mac</b>, with shortcuts for every section, Back and Forward, and Settings.</>,
        <><b>A local model or ChatGPT</b> for the assistant, and <b>agents</b> such as Claude through MCP, only on your computer.</>,
        <><b>Updates</b>: Install verified updates from the app. Debian / Ubuntu and Arch packages are updated through the package manager.</>,
      ],
      alt: "Discophile on a Mac, playing Discophile by Mina Sol with synced lyrics",
    },
    downloads: {
      mac: { title: "Mac", needs: "macOS 11 or later, Apple silicon and Intel", body: "Signed and notarized by Apple. Open the disk image and drag Discophile to Applications." },
      windows: { title: "Windows", needs: "Windows 10 or later, 64-bit", body: <>Extract the zip and run <code>Discophile.exe</code>.</> },
      linux: { title: "Linux", needs: "x86_64 · Ubuntu 22.04 or equivalent, glibc 2.35+", body: <>Choose a Debian / Ubuntu package, a portable AppImage, or the tar archive. The download window includes instructions for each format.</> },
      aur: <>On Arch Linux, install <a href={AUR_URL}>discophile-bin</a> from the AUR:</>,
      button: { mac: "Download for Mac", windows: "Download for Windows", linux: "Download for Linux" },
      fallback: "Releases on GitHub",
    },
    web: {
      title: "Any browser",
      needs: "A server you host, with Docker or Kubernetes",
      body: "The web player is the same app compiled for the browser. Host it next to your music server and listen from any computer, with a built-in proxy so you don't have to set up CORS.",
      facts: [
        "Your credentials stay in that browser's storage.",
        "It streams only: downloads for offline listening and ChatGPT sign-in are desktop and iPhone features.",
      ],
      link: "Host the web player",
      alt: "The Discophile web player in a browser, showing the album grid",
    },
    compare: {
      title: "Side by side.",
      caption: "What Discophile can do on each device",
      head: ["", "iPhone", "Desktop", "Web player"],
      rows: [
        ["Navidrome, Subsonic and Jellyfin", "Yes", "Yes", "Yes"],
        ["Audiobookshelf and internet radio", "Yes", "Yes", "Yes"],
        ["Listen Now, statistics and synced lyrics", "Yes", "Yes", "Yes"],
        ["Equalizer and ReplayGain", "Yes", "Yes", "Yes"],
        ["Offline downloads", "Yes", "Yes", "No"],
        ["Music assistant", "Apple Intelligence or ChatGPT", "Local model or ChatGPT", "Local model"],
        ["Siri, dictation and Sport mode", "Yes", "No", "No"],
        ["Agents through MCP", "No", "Yes", "No"],
        ["Passwords kept in", "iOS Keychain", "System keychain", "Browser storage"],
      ],
    },
    footer: <>Release notes, checksums and every file are on <a href={RELEASES_URL}>GitHub</a>, where you can also read the <a href={REPO_URL}>source code</a> of the desktop and web apps, free and open source under the MIT license.</>,
  },
  it: {
    title: "Ovunque tu ascolti.",
    lede: (version) => `Su iPhone, Android, Mac, Windows, Linux o in qualsiasi browser da un server che ospiti tu.${version ? ` Ultima release: ${version}.` : ""}`,
    iphone: {
      title: "iPhone",
      needs: "iOS 17 o versioni successive",
      body: "Un'app nativa in SwiftUI, a casa sua su iOS, con musica, audiolibri e radio sempre a portata di pollice.",
      facts: [
        <><b>Apple Intelligence</b> fa funzionare l&apos;assistente musicale sull&apos;iPhone, senza che nulla lasci il dispositivo.</>,
        <><b>Siri, la schermata di blocco e AirPlay</b> controllano la riproduzione, e la dettatura trasforma la tua voce in una richiesta.</>,
        <><b>La modalità sport</b> salva impostazioni per corsa, camminata, bici e palestra, da avviare con Comandi Rapidi.</>,
      ],
      soon: "In attesa dell'approvazione dell'App Store. Nel frattempo usa l'IPA per il sideloading, quando disponibile qui sotto, oppure richiedi un invito a TestFlight.",
      alt: "Discophile su iPhone mentre riproduce Discophile di Mina Sol",
    },
    android: {
      title: "Android",
      needs: "Android 8 o versioni successive",
      body: "Il tuo server musicale sul telefono Android. Installa direttamente l'APK firmato, senza Google Play, con la stessa versione degli altri player Discophile.",
    },
    desktop: {
      title: "Mac, Windows e Linux",
      body: "Lo stesso Discophile in una finestra pensata per tastiera e schermo grande: la libreria nella barra laterale, il player sempre a portata di mano, e coda, testi, Informazioni e assistente accanto a quello che stai sfogliando.",
      facts: [
        <><b>Controlli multimediali di sistema</b>: In riproduzione su macOS, l&apos;overlay multimediale su Windows e MPRIS su Linux, più un menu nell&apos;area di notifica per continuare ad ascoltare a finestra chiusa.</>,
        <><b>Una barra dei menu completa sul Mac</b>, con scorciatoie per ogni sezione, Indietro e Avanti, e Impostazioni.</>,
        <><b>Un modello locale o ChatGPT</b> per l&apos;assistente, e <b>agenti</b> come Claude tramite MCP, solo sul tuo computer.</>,
        <><b>Aggiornamenti</b>: Installa gli aggiornamenti verificati dall’app. I pacchetti Debian / Ubuntu e Arch si aggiornano con il gestore di pacchetti.</>,
      ],
      alt: "Discophile su un Mac mentre riproduce Discophile di Mina Sol con i testi sincronizzati",
    },
    downloads: {
      mac: { title: "Mac", needs: "macOS 11 o successivi, Apple silicon e Intel", body: "Firmato e autenticato da Apple. Apri l'immagine disco e trascina Discophile in Applicazioni." },
      windows: { title: "Windows", needs: "Windows 10 o successivi, 64 bit", body: <>Estrai lo zip e avvia <code>Discophile.exe</code>.</> },
      linux: { title: "Linux", needs: "x86_64 · Ubuntu 22.04 o equivalente, glibc 2.35+", body: <>Scegli il pacchetto Debian / Ubuntu, l&apos;AppImage portatile o l&apos;archivio tar. Nella finestra di download trovi le istruzioni per ogni formato.</> },
      aur: <>Su Arch Linux installa <a href={AUR_URL}>discophile-bin</a> da AUR:</>,
      button: { mac: "Scarica per Mac", windows: "Scarica per Windows", linux: "Scarica per Linux" },
      fallback: "Release su GitHub",
    },
    web: {
      title: "Qualsiasi browser",
      needs: "Un server che ospiti tu, con Docker o Kubernetes",
      body: "Il player web è la stessa app compilata per il browser. Ospitalo accanto al tuo server musicale e ascolta da qualsiasi computer, con un proxy integrato che ti evita di configurare CORS.",
      facts: [
        "Le tue credenziali restano nello storage di quel browser.",
        "Funziona solo in streaming: i download per l'ascolto offline e l'accesso a ChatGPT sono funzioni del desktop e dell'iPhone.",
      ],
      link: "Ospita il player web",
      alt: "Il player web di Discophile in un browser, con la griglia degli album",
    },
    compare: {
      title: "Fianco a fianco.",
      caption: "Cosa può fare Discophile su ogni dispositivo",
      head: ["", "iPhone", "Desktop", "Player web"],
      rows: [
        ["Navidrome, Subsonic e Jellyfin", "Sì", "Sì", "Sì"],
        ["Audiobookshelf e radio online", "Sì", "Sì", "Sì"],
        ["Ascolta, statistiche e testi sincronizzati", "Sì", "Sì", "Sì"],
        ["Equalizzatore e ReplayGain", "Sì", "Sì", "Sì"],
        ["Download offline", "Sì", "Sì", "No"],
        ["Assistente musicale", "Apple Intelligence o ChatGPT", "Modello locale o ChatGPT", "Modello locale"],
        ["Siri, dettatura e modalità sport", "Sì", "No", "No"],
        ["Agenti tramite MCP", "No", "Sì", "No"],
        ["Password conservate in", "Portachiavi di iOS", "Portachiavi di sistema", "Storage del browser"],
      ],
    },
    footer: <>Note di rilascio, checksum e tutti i file sono su <a href={RELEASES_URL}>GitHub</a>, dove puoi anche leggere il <a href={REPO_URL}>codice sorgente</a> delle app desktop e web, gratuite e open source con licenza MIT.</>,
  },
};


export async function DownloadPage({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  const release = await latestRelease();
  const d = c.downloads;

  const card = (platform: DesktopPlatform, extra?: ReactNode) => {
    const file = release?.files[platform];
    return (
      <section className="download-card" id={platform} aria-labelledby={`${platform}-title`}>
        <h3 id={`${platform}-title`}>{d[platform].title}</h3>
        <p className="needs">{d[platform].needs}</p>
        <p>{d[platform].body}</p>
        <div className="card-actions">
          <DownloadButton platform={platform} labels={{ any: d.fallback, ...d.button }} />
          {file && <p className="fineprint file">{file.name} · {megabytes(file.size)}</p>}
          {extra}
        </div>
      </section>
    );
  };

  return (
    <>
      <main id="main" tabIndex={-1}>
        <header className="page-head">
          <h1>{c.title}</h1>
          <p className="lede">{c.lede(release?.version ?? null)}</p>
        </header>

        <section className="feature device" id="iphone" aria-labelledby="iphone-title">
          <div className="feature-copy">
            <h2 id="iphone-title">{c.iphone.title}</h2>
            <p className="needs">{c.iphone.needs}</p>
            <p>{c.iphone.body}</p>
            <ul className="facts">{c.iphone.facts.map((fact, i) => <li key={i}>{fact}</li>)}</ul>
            <p className="fineprint">{c.iphone.soon}</p>
            <MobileDownload locale={locale} platform="ios" release={release} />
            <div className="actions"><TestFlightButton className="button" /></div>
          </div>
          <div className="phones single"><Screen name="now-playing" alt={c.iphone.alt} /></div>
        </section>

        <section className="mobile-device device" id="android" aria-labelledby="android-title">
          <div className="feature-copy">
            <h2 id="android-title">{c.android.title}</h2>
            <p className="needs">{c.android.needs}</p>
            <p>{c.android.body}</p>
          </div>
          <MobileDownload locale={locale} platform="android" release={release} />
        </section>

        <section className="desk device" id="desktop" aria-labelledby="desktop-title">
          <div className="desk-intro">
            <div className="feature-copy">
              <h2 id="desktop-title">{c.desktop.title}</h2>
              <p>{c.desktop.body}</p>
              <ul className="facts">{c.desktop.facts.map((fact, i) => <li key={i}>{fact}</li>)}</ul>
            </div>
            <Window name="now-playing" alt={c.desktop.alt} />
          </div>
          <div className="download-grid desk-downloads">
            {card("mac")}
            {card("windows")}
            {card("linux", (
              <>
                <p className="aur">{d.aur}</p>
                <pre><code>yay -S discophile-bin</code></pre>
              </>
            ))}
          </div>
        </section>

        <ConnectOverview locale={locale} />

        <section className="feature device" id="browser" aria-labelledby="browser-title">
          <div className="feature-copy">
            <h2 id="browser-title">{c.web.title}</h2>
            <p className="needs">{c.web.needs}</p>
            <p>{c.web.body}</p>
            <ul className="facts">{c.web.facts.map((fact) => <li key={fact}>{fact}</li>)}</ul>
            <div className="actions"><a className="button primary" href={localePath(locale, "/self-hosting")}>{c.web.link}</a></div>
          </div>
          <figure className="browser">
            <div className="browser-bar" aria-hidden="true">
              <span className="browser-lights"><span /><span /><span /></span>
              <span className="browser-nav">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="m15 18-6-6 6-6" /></svg>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="m9 18 6-6-6-6" /></svg>
              </span>
              <span className="browser-address">
                <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor"><path d="M17 10V7a5 5 0 0 0-10 0v3H5v12h14V10Zm-8 0V7a3 3 0 0 1 6 0v3Z" /></svg>
                music.example.com
              </span>
              <span className="browser-nav" />
            </div>
            <WebShot name="albums" alt={c.web.alt} />
          </figure>
        </section>

        <section className="compare" aria-labelledby="compare-title">
          <h2 id="compare-title">{c.compare.title}</h2>
          <div className="table-scroll">
            <table>
              <caption className="sr-only">{c.compare.caption}</caption>
              <thead><tr>{c.compare.head.map((cell, i) => i === 0 ? <td key={i} /> : <th key={i} scope="col">{cell}</th>)}</tr></thead>
              <tbody>
                {c.compare.rows.map(([label, ...cells]) => (
                  <tr key={label}>
                    <th scope="row">{label}</th>
                    {cells.map((cell, i) => <td key={i} data-label={c.compare.head[i + 1]} data-value={cell === "No" ? "no" : cell === "Yes" || cell === "Sì" ? "yes" : undefined}>{cell}</td>)}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="downloads-footer">{c.footer}</p>
        </section>
      </main>
      <SiteFooter locale={locale} note={ui[locale].screenshotsNote} />
    </>
  );
}
