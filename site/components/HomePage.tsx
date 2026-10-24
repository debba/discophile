import { DesktopShot, PhoneShot } from "@/components/DesktopShot";
import type { ReactNode } from "react";
import { DownloadButton } from "@/components/Download";
import { HeroVideo } from "@/components/HeroVideo";
import { Icon, type IconName } from "@/components/Icon";
import { SiteFooter } from "@/components/SiteFooter";
import { StructuredData } from "@/components/StructuredData";
import { DeviceShots, type Shot } from "@/components/Shots";
import { TestFlightButton } from "@/components/TestFlight";
import { localePath, ui, type Locale } from "@/lib/i18n";

type Copy = {
  title: string;
  lede: string;
  download: { any: string; mac: string; windows: string; linux: string };
  free: ReactNode;
  soon: ReactNode;
  testflight: string;
  listen: { title: string; body: ReactNode[]; iphone: Shot[]; desktop: Shot[] };
  assistant: { title: string; body: ReactNode[]; facts: ReactNode[]; iphone: Shot[]; desktop: Shot[] };
  audiobooks: { title: string; body: string; facts: string[]; iphone: Shot[]; desktop: Shot[] };
  everywhere: { title: string; body: string; cards: { title: string; body: string; link: string; href: string }[]; features: string };
  details: { title: string; items: [string, string, string?][] };
  privacy: { label: string; text: string };
  release: { title: string; body: string; servers: string; serverNames: string[]; devices: string; question: string };
  sponsor: { title: string; body: string; button: string };
};

const COPY: Record<Locale, Copy> = {
  en: {
    title: "Your music server, played like a record collection.",
    lede: "Discophile plays your Navidrome, Subsonic and Jellyfin libraries on iPhone, Mac, Windows, Linux and in the browser. Audiobookshelf audiobooks and internet radio live in the same app. No Discophile account, no tracking.",
    download: { any: "Get Discophile", mac: "Download for Mac", windows: "Download for Windows", linux: "Download for Linux" },
    free: <><b>Free for Mac, Windows and Linux.</b> Open source, signed and ready to download.</>,
    soon: <><b>Coming soon to the App Store.</b> For iPhone with iOS 17 or later, in English and Italian.</>,
    testflight: "Try the iPhone beta",
    listen: {
      title: "Something familiar. Something unexpected.",
      body: [
        <>Listen Now opens with mixes built from your own listening. <b>Worth another listen</b> brings back songs you haven&apos;t played in a month, <b>Still to discover</b> finds the ones you never played, and <b>Have 25 minutes?</b> fits a short break.</>,
        <><b>Choose a record for tonight</b> suggests three whole albums for 25, 45 or 60 minutes, then plays the one you pick from first track to last.</>,
      ],
      iphone: [["listen-now", "Listen Now with discovery mixes and album suggestions"], ["album", "An album page with Play, Shuffle and the track list"], ["now-playing", "Now Playing with large artwork, timeline and transport controls"]],
      desktop: [["listen-now", "Listen Now on the desktop, with mixes and album suggestions"]],
    },
    assistant: {
      title: "Ask for a playlist. Get songs you own.",
      body: [
        "The music assistant turns a sentence into a selection from your library, so every song it suggests is one you can play right now. Use Apple Intelligence on the iPhone, a local model on the desktop, or connect your ChatGPT account for bigger requests.",
        <>Edits such as removing tracks or renaming a playlist open a review first. Nothing changes on your server until you confirm.</>,
      ],
      facts: [
        <><b>On this iPhone</b> runs Apple&apos;s local model on iOS 26 with Apple Intelligence.</>,
        <><b>ChatGPT</b> receives your request and song metadata, never audio, passwords or listening history.</>,
        <><b>About</b> tells the story behind an artist or album, from your server&apos;s notes and, if you allow it, MusicBrainz and Wikipedia.</>,
      ],
      iphone: [["ai-local", "A playlist for a rainy Sunday generated on the iPhone, with Play and Review playlist"], ["ai-chatgpt", "Apple Intelligence on the iPhone explaining why Discophile by Mina Sol is catchy"], ["ai-chatgpt-songs", "A selection of songs like Discophile, picked on the iPhone from the library"]],
      desktop: [["assistant", "The music assistant beside Listen Now, proposing a playlist"]],
    },
    audiobooks: {
      title: "Audiobooks pick up where you stopped.",
      body: "Connect Audiobookshelf next to your music, or on its own. Positions sync with the server every few seconds and keep working offline, so you can move between your phone, your computer and the web without hunting for your place.",
      facts: [
        "Speed from 0.5x to 4x, chapter timelines and exact seeking.",
        "Sleep timer, including at the end of the chapter.",
        "Bookmarks, and whole books downloaded for offline listening.",
      ],
      iphone: [["audiobook", "An audiobook page with Continue listening and chapters"], ["audiobook-player", "The audiobook player on chapter 13, with skip buttons and speed"]],
      desktop: [["audiobooks", "The audiobook library on the desktop"], ["audiobook-player", "An audiobook playing on chapter 13 of 34"]],
    },
    everywhere: {
      title: "One collection, every screen.",
      body: "The same mixes, library, assistant and audiobooks, wherever you listen.",
      cards: [
        { title: "iPhone", body: "Native on iOS, with Siri, the Lock Screen, AirPlay and an assistant that runs on the device.", link: "Discophile for iPhone", href: "/download#iphone" },
        { title: "Mac, Windows and Linux", body: "A window made for a big screen, with media keys, a tray menu, an equalizer curve and agents.", link: "Download for desktop", href: "/download#desktop" },
        { title: "Any browser", body: "Host the web player next to your server with Docker or Kubernetes, and listen from anywhere.", link: "Host the web player", href: "/self-hosting" },
      ],
      features: "See everything Discophile does",
    },
    details: {
      title: "And the parts you notice later.",
      items: [
        ["Gapless and crossfade", "Albums play without pauses. Crossfade from 1 to 12 seconds, kept gapless inside an album."],
        ["Equalizer", "Ten bands, or a headphone correction from AutoEq. ReplayGain and a volume limit too."],
        ["Lyrics", "Synced lyrics follow the song and jump to a verse when you tap it."],
        ["Podcasts and radio", "Subscribe with an Apple Podcasts or Spotify link, and save internet radio stations."],
        ["Sport mode", "Presets for running, walking, cycling and the gym, started from Shortcuts.", "iPhone"],
        ["Media keys and tray", "Play and skip from your keyboard, the system media controls or the tray, with the window closed.", "Desktop"],
      ],
    },
    privacy: {
      label: "Privacy",
      text: "There is no Discophile server. Your passwords stay in your device's keychain, your listening history stays on your device, and the app has no analytics.",
    },
    release: {
      title: "Get Discophile.",
      body: "Free for Mac, Windows and Linux today, and coming soon to the App Store for iPhone. Bring your own server: it works with the music and audiobooks you already host.",
      servers: "Supported servers",
      serverNames: ["Navidrome", "Subsonic and OpenSubsonic", "Jellyfin", "Audiobookshelf", "Internet radio"],
      devices: "All downloads and requirements",
      question: "Have a question or an idea? Write to",
    },
    sponsor: {
      title: "Help Discophile grow.",
      body: "If Discophile is useful to you, you can support me on GitHub Sponsors. Even a small contribution helps me spend more time on fixes, accessibility and new features.",
      button: "Support on GitHub Sponsors",
    },
  },
  it: {
    title: "La tua musica. Il tuo mondo. Premi play.",
    lede: "La tua libreria merita il palco. Ascolta la musica di Navidrome, Subsonic e Jellyfin su iPhone, Mac, Windows, Linux e nel browser, ritrova i tuoi audiolibri di Audiobookshelf e accendi le radio che ami. Tutto in Discophile, senza creare un altro account e senza tracciamento.",
    download: { any: "Scarica Discophile", mac: "Scarica per Mac", windows: "Scarica per Windows", linux: "Scarica per Linux" },
    free: <><b>Gratis per Mac, Windows e Linux.</b> Open source, firmato e pronto da scaricare.</>,
    soon: <><b>Presto sull&apos;App Store.</b> Per iPhone con iOS 17 o versioni successive, in italiano e in inglese.</>,
    testflight: "Prova la beta per iPhone",
    listen: {
      title: "Riscopri i tuoi pezzi. Trova la prossima ossessione.",
      body: [
        <>Apri Ascolta e lasciati ispirare da mix basati sui tuoi ascolti. <b>Da riascoltare</b> ti ripropone i brani che non ascolti da un mese, <b>Ancora da scoprire</b> trova quelli che non hai mai ascoltato e <b>Hai 25 minuti?</b> dà la colonna sonora alla tua pausa.</>,
        <><b>Scegli un disco per stasera</b> ti propone tre album interi da 25, 45 o 60 minuti, poi riproduce quello che scegli dalla prima all&apos;ultima traccia.</>,
      ],
      iphone: [["listen-now", "La schermata Ascolta con i mix di scoperta e gli album suggeriti"], ["album", "La pagina di un album con Riproduci, Casuale e l'elenco delle tracce"], ["now-playing", "In riproduzione, con copertina grande, barra di avanzamento e controlli"]],
      desktop: [["listen-now", "Ascolta sul desktop, con i mix e gli album suggeriti"]],
    },
    assistant: {
      title: "Tu dai l’idea. La tua musica fa il resto.",
      body: [
        "Descrivi l’atmosfera, un’occasione o quello che ti va di ascoltare. L’assistente sceglie i brani dalla tua libreria e li mette subito a portata di play. Usa Apple Intelligence sull’iPhone, un modello locale sul desktop, oppure collega il tuo account ChatGPT.",
        <>Vuoi togliere brani o rinominare una playlist? Prima ti viene mostrato un riepilogo, e sul tuo server non cambia nulla finché non confermi.</>,
      ],
      facts: [
        <><b>Su questo iPhone</b> usa il modello locale di Apple su iOS 26 con Apple Intelligence.</>,
        <><b>ChatGPT</b> riceve la tua richiesta e i metadati dei brani, mai audio, password o cronologia di ascolto.</>,
        <><b>Informazioni</b> racconta la storia di un artista o di un album, dalle note del tuo server e, se lo consenti, da MusicBrainz e Wikipedia.</>,
      ],
      iphone: [["ai-local", "Una playlist per una domenica di pioggia creata sull'iPhone, con Riproduci e Rivedi playlist"], ["ai-chatgpt", "Apple Intelligence sull'iPhone spiega perché Discophile di Mina Sol è orecchiabile"], ["ai-chatgpt-songs", "Una selezione di brani simili a Discophile, scelti sull'iPhone dalla libreria"]],
      desktop: [["assistant", "L'assistente musicale accanto ad Ascolta, che propone una playlist"]],
    },
    audiobooks: {
      title: "Un altro capitolo. Poi un altro ancora.",
      body: "Porta con te le storie di Audiobookshelf, insieme alla musica o anche da sole. Discophile salva il punto di ascolto anche offline e lo sincronizza con il server ogni pochi secondi. Passa dall’iPhone al computer o al web e riprendi da dove eri rimasto.",
      facts: [
        "Velocità di riproduzione da 0,5x a 4x, capitoli sulla barra di avanzamento e salti precisi nel punto che vuoi.",
        "Timer di spegnimento, anche a fine capitolo.",
        "Segnalibri e libri interi da scaricare per ascoltarli anche offline.",
      ],
      iphone: [["audiobook", "La pagina di un audiolibro con Continua ad ascoltare e i capitoli"], ["audiobook-player", "Il player degli audiolibri al capitolo 13, con i pulsanti per saltare e la velocità"]],
      desktop: [["audiobooks", "La libreria degli audiolibri sul desktop"], ["audiobook-player", "Un audiolibro in riproduzione al capitolo 13 di 34"]],
    },
    everywhere: {
      title: "Una collezione, ogni schermo.",
      body: "Gli stessi mix, la stessa libreria, lo stesso assistente e gli stessi audiolibri, ovunque tu ascolti.",
      cards: [
        { title: "iPhone", body: "Nativo su iOS, con Siri, la schermata di blocco, AirPlay e un assistente che funziona sul dispositivo.", link: "Discophile per iPhone", href: "/download#iphone" },
        { title: "Mac, Windows e Linux", body: "Una finestra pensata per lo schermo grande, con tasti multimediali, area di notifica, curva dell'equalizzatore e agenti.", link: "Scarica per desktop", href: "/download#desktop" },
        { title: "Qualsiasi browser", body: "Ospita il player web accanto al tuo server con Docker o Kubernetes, e ascolta da dove vuoi.", link: "Ospita il player web", href: "/self-hosting" },
      ],
      features: "Scopri tutto quello che fa Discophile",
    },
    details: {
      title: "Il bello si sente anche nei dettagli.",
      items: [
        ["Senza pause e dissolvenza", "Gli album si ascoltano senza interruzioni tra un brano e l'altro. Dissolvenza incrociata da 1 a 12 secondi, che dentro un album lascia il posto alla riproduzione senza pause."],
        ["Equalizzatore", "Dieci bande, oppure la correzione per le tue cuffie con AutoEq. Ci sono anche ReplayGain e un limite di volume."],
        ["Testi", "Segui le parole a tempo con la musica. Tocca una strofa per riascoltarla subito."],
        ["Podcast e radio", "Aggiungi i podcast con un link di Apple Podcasts o Spotify e salva le tue radio online preferite."],
        ["Modalità sport", "Impostazioni pronte per corsa, camminata, bici e palestra, da avviare con Comandi Rapidi.", "iPhone"],
        ["Tasti multimediali e area di notifica", "Riproduci e salta dalla tastiera, dai controlli multimediali del sistema o dall'area di notifica, anche a finestra chiusa.", "Desktop"],
      ],
    },
    privacy: {
      label: "Privacy",
      text: "Discophile non ha un server. Le password restano nel portachiavi del tuo dispositivo, la cronologia di ascolto resta sul tuo dispositivo e nell'app non ci sono strumenti di analisi.",
    },
    release: {
      title: "Il prossimo ascolto comincia qui.",
      body: "Gratis per Mac, Windows e Linux da oggi, e presto sull’App Store per iPhone. Porta il tuo server: funziona con la musica e gli audiolibri che ospiti già.",
      servers: "Server supportati",
      serverNames: ["Navidrome", "Subsonic e OpenSubsonic", "Jellyfin", "Audiobookshelf", "Radio online"],
      devices: "Tutti i download e i requisiti",
      question: "Hai una domanda o un'idea? Scrivi a",
    },
    sponsor: {
      title: "Dai una spinta al prossimo giro.",
      body: "Se Discophile ti è utile, puoi sostenermi su GitHub Sponsors. Anche un piccolo contributo mi permette di dedicare più tempo a correzioni, accessibilità e nuove funzioni.",
      button: "Sostieni su GitHub Sponsors",
    },
  },
};

const DETAIL_ICONS: IconName[] = ["gapless", "equalizer", "lyrics", "radio", "sport", "keys"];

const DEVICE_ICONS: ReactNode[] = [
  <path key="phone" d="M8 2h8a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2Zm3 17h2" />,
  <path key="desktop" d="M4 4h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1Zm4 16h8m-4-4v4" />,
  <path key="browser" d="M3 5h18v14H3zM3 9h18M6 7h.01M9 7h.01" />,
];

export function HomePage({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  const labels = ui[locale].shots;
  const devices = localePath(locale, "/download");
  const group = (title: string) => ({ ...labels, group: `${labels.group} ${title}` });

  return (
    <>
      <StructuredData locale={locale} />
      <main id="main" tabIndex={-1}>
        <section className="hero">
          <div className="hero-copy">
            <h1>{c.title}</h1>
            <p className="lede">{c.lede}</p>
            <div className="actions">
              <DownloadButton labels={c.download} onIOS={<TestFlightButton />} />
              <HeroVideo locale={locale} />
            </div>
            <p className="fineprint soon">
              <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2" fill="none" stroke="currentColor" strokeWidth="2" /><path d="M8 20h8M12 16v4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>
              <span>{c.free}</span>
            </p>
            <p className="fineprint soon">
              <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="2" /><path d="M12 7v5l3 2" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>
              <span>{c.soon} <TestFlightButton className="link-button">{c.testflight}</TestFlightButton>.</span>
            </p>
          </div>
          <div className="turntable stage" aria-hidden="true">
            <div className="deck">
              <div className="record"><div className="label" /></div>
              <svg className="tonearm" viewBox="0 0 1000 1000">
                <g className="arm">
                  <path d="M902 436L823 657" fill="none" stroke="#f6e9ef" strokeWidth="40" strokeLinecap="round" />
                  <path d="M923 75L902 436" fill="none" stroke="#f6e9ef" strokeWidth="40" strokeLinecap="round" />
                  <rect x="757" y="622" width="132" height="70" rx="18" fill="#f6e9ef" transform="rotate(110 823 657)" />
                  <circle cx="923" cy="75" r="78" fill="#f6e9ef" />
                  <circle cx="923" cy="75" r="46" fill="none" stroke="#f7789c" strokeOpacity=".6" strokeWidth="7" />
                  <circle cx="923" cy="75" r="25" fill="#c72d62" />
                </g>
              </svg>
            </div>
            <figure className="macbook hero-macbook">
              <div className="macbook-lid">
                <div className="macbook-screen">
                  <div className="macbook-menubar">
                    <b>Discophile</b><span>File</span><span>Controls</span><span>Go</span><span>Window</span><span>Help</span>
                    <span className="macbook-clock">9:41</span>
                  </div>
                  <DesktopShot name="now-playing" alt="" eager />
                </div>
              </div>
              <div className="macbook-base" />
            </figure>
            <figure className="phone hero-phone">
              <PhoneShot name="now-playing" alt="" eager />
            </figure>
          </div>
        </section>

        <section className="feature" id="listen" aria-labelledby="listen-title">
          <div className="feature-copy">
            <h2 id="listen-title">{c.listen.title}</h2>
            {c.listen.body.map((text, i) => <p key={i}>{text}</p>)}
          </div>
          <DeviceShots labels={group(c.listen.title)} iphone={c.listen.iphone} desktop={c.listen.desktop} layout="trio" />
        </section>

        <section className="feature assistant" id="assistant" aria-labelledby="assistant-title">
          <div className="feature-copy">
            <h2 id="assistant-title">{c.assistant.title}</h2>
            {c.assistant.body.map((text, i) => <p key={i}>{text}</p>)}
            <ul className="facts">
              {c.assistant.facts.map((fact, i) => <li key={i}>{fact}</li>)}
            </ul>
          </div>
          <DeviceShots labels={group(c.assistant.title)} iphone={c.assistant.iphone} desktop={c.assistant.desktop} layout="fan" />
        </section>

        <section className="feature reverse" id="audiobooks" aria-labelledby="audiobooks-title">
          <div className="feature-copy">
            <h2 id="audiobooks-title">{c.audiobooks.title}</h2>
            <p>{c.audiobooks.body}</p>
            <ul className="facts">
              {c.audiobooks.facts.map((fact) => <li key={fact}>{fact}</li>)}
            </ul>
          </div>
          <DeviceShots labels={group(c.audiobooks.title)} iphone={c.audiobooks.iphone} desktop={c.audiobooks.desktop} layout="pair" />
        </section>

        <section className="everywhere" id="devices" aria-labelledby="everywhere-title">
          <h2 id="everywhere-title">{c.everywhere.title}</h2>
          <p>{c.everywhere.body}</p>
          <ul className="device-cards">
            {c.everywhere.cards.map((card, i) => (
              <li key={card.title}>
                <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{DEVICE_ICONS[i]}</svg>
                <h3>{card.title}</h3>
                <p>{card.body}</p>
                <a href={localePath(locale, card.href)}>{card.link}<span aria-hidden="true"> →</span></a>
              </li>
            ))}
          </ul>
          <a className="button" href={localePath(locale, "/features")}>{c.everywhere.features}</a>
        </section>

        <section className="details" aria-labelledby="details-title">
          <h2 id="details-title">{c.details.title}</h2>
          <dl>
            {c.details.items.map(([term, text, tag], i) => (
              <div key={term}>
                <dt><Icon name={DETAIL_ICONS[i]} />{term}{tag && <span className="badge">{tag}</span>}</dt>
                <dd>{text}</dd>
              </div>
            ))}
          </dl>
        </section>

        <section className="privacy" aria-label={c.privacy.label}>
          <p>{c.privacy.text}</p>
        </section>

        <section className="release" id="release" aria-labelledby="release-title">
          <img src="/assets/icon-192.png" alt="" width={96} height={96} />
          <h2 id="release-title">{c.release.title}</h2>
          <p>{c.release.body}</p>
          <ul className="servers" aria-label={c.release.servers}>
            {c.release.serverNames.map((server) => <li key={server}>{server}</li>)}
          </ul>
          <div className="actions">
            {/* On iPhone the beta button below is the only action. */}
            <DownloadButton labels={c.download} hideOnIOS />
            <TestFlightButton className="button">{c.testflight}</TestFlightButton>
          </div>
          <p className="fineprint"><a href={devices}>{c.release.devices}</a></p>
          <p className="fineprint">{c.release.question} <a href="mailto:andrea@debbaweb.it?subject=Discophile">andrea@debbaweb.it</a>.</p>
        </section>

        <section className="sponsor" aria-labelledby="sponsor-title">
          <div>
            <h2 id="sponsor-title">{c.sponsor.title}</h2>
            <p>{c.sponsor.body}</p>
          </div>
          <a className="button" href="https://github.com/sponsors/debba">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1.1-1.1a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8Z" /></svg>
            {c.sponsor.button}
          </a>
        </section>
      </main>

      <SiteFooter locale={locale} note={ui[locale].screenshotsNote} />
    </>
  );
}
