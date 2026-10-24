import type { ReactNode } from "react";
import { HeroVideo } from "@/components/HeroVideo";
import { SiteFooter } from "@/components/SiteFooter";
import { TestFlightButton } from "@/components/TestFlight";
import type { Locale } from "@/lib/i18n";

type Copy = {
  title: string;
  lede: string;
  soon: string;
  fineprint: string;
  listen: { title: string; body: ReactNode[]; screens: [string, string][] };
  library: { title: string; body: ReactNode; region: string; screens: [string, string, string][] };
  assistant: { title: string; body: ReactNode[]; facts: ReactNode[]; screens: [string, string][] };
  audiobooks: { title: string; body: string; facts: string[]; screens: [string, string][] };
  details: { title: string; items: [string, string][] };
  privacy: { label: string; text: string };
  release: { title: string; body: string; servers: string; serverNames: string[]; question: string };
  sponsor: { title: string; body: string; button: string };
  footerNote: string;
};

const COPY: Record<Locale, Copy> = {
  en: {
    title: "Your music server, played like a record collection.",
    lede: "Discophile is a native iPhone player for Navidrome, Subsonic and Jellyfin libraries. Audiobookshelf audiobooks and internet radio live in the same app. No Discophile account, no tracking.",
    soon: "Coming soon to the App Store",
    fineprint: "For iPhone with iOS 17 or later, in English and Italian.",
    listen: {
      title: "Something familiar. Something unexpected.",
      body: [
        <>Listen Now opens with mixes built from your own history on this iPhone. <b>Worth another listen</b> brings back songs you haven&apos;t played in a month, <b>Still to discover</b> finds the ones you never played, and <b>Have 25 minutes?</b> fits a short break.</>,
        <><b>Choose a record for tonight</b> suggests three whole albums for 25, 45 or 60 minutes, then plays the one you pick from first track to last.</>,
      ],
      screens: [
        ["listen-now", "Listen Now with discovery mixes and album suggestions"],
        ["album", "An album page with Play, Shuffle and the track list"],
        ["now-playing", "Now Playing with large artwork, timeline and transport controls"],
      ],
    },
    library: {
      title: "The whole shelf, a thumb away.",
      body: <>Browse albums, artists, songs and playlists, search everything from one field, and download anything for the flight. Switch on <b>Downloaded only</b> and every view shows just what is saved on the phone.</>,
      region: "Library screenshots, scrollable",
      screens: [
        ["library-albums", "Library albums grid", "Albums"],
        ["library-artists", "Library artists list", "Artists"],
        ["search", "Search results for artists, albums and songs", "Search"],
        ["statistics", "Listening statistics for the last 30 days", "Listening statistics"],
        ["statistics-charts", "Minutes listened per day and top songs", "Top songs"],
      ],
    },
    assistant: {
      title: "Ask for a playlist. Get songs you own.",
      body: [
        "The music assistant turns a sentence into a selection from your library, so every song it suggests is one you can play right now. Use Apple Intelligence on the iPhone, with nothing leaving the device, or connect your ChatGPT account for bigger requests.",
        <>Edits such as removing tracks or renaming a playlist open a review first. Nothing changes on your server until you tap <b>Confirm changes</b>.</>,
      ],
      facts: [
        <><b>On this iPhone</b> runs Apple&apos;s local model on iOS 26 with Apple Intelligence.</>,
        <><b>ChatGPT</b> receives your request and song metadata, never audio, passwords or listening history.</>,
        <><b>Ask your curator</b> builds half an hour for a quiet evening or for focus.</>,
      ],
      screens: [
        ["ai-local", "A playlist generated on the iPhone, with Play and Create playlist"],
        ["ai-chatgpt", "ChatGPT explaining why a song is catchy and picking similar songs"],
        ["curator", "The curator screen with provider, model and listening occasion"],
      ],
    },
    audiobooks: {
      title: "Audiobooks pick up where you stopped.",
      body: "Connect Audiobookshelf next to your music, or on its own. Positions sync with the server every few seconds and keep working offline, so you can switch between the phone and the web without hunting for your place.",
      facts: [
        "Speed from 0.5x to 4x, chapter timelines and exact seeking.",
        "Sleep timer, including at the end of the chapter.",
        "Bookmarks, and whole books downloaded for offline listening.",
      ],
      screens: [
        ["audiobook", "An audiobook page with Continue listening and chapters"],
        ["audiobook-player", "The audiobook player on chapter 1 with skip buttons and speed"],
      ],
    },
    details: {
      title: "And the parts you notice later.",
      items: [
        ["Gapless and crossfade", "Albums play without pauses. Crossfade from 1 to 12 seconds, kept gapless inside an album."],
        ["Equalizer", "Ten bands, or a headphone correction from AutoEq. ReplayGain and a volume limit too."],
        ["Lyrics", "Synced lyrics follow the song and jump to a verse when you tap it."],
        ["Podcasts and radio", "Subscribe with an Apple Podcasts or Spotify link, and save internet radio stations."],
        ["Sport mode", "Presets for running, walking, cycling and the gym, started from Shortcuts."],
        ["Siri and Lock Screen", "Ask Siri for an album, and control playback from the Lock Screen, headphones or AirPlay."],
      ],
    },
    privacy: {
      label: "Privacy",
      text: "There is no Discophile server. Your passwords stay in the iOS Keychain, your listening history stays on your iPhone, and the app has no analytics.",
    },
    release: {
      title: "Try it now on TestFlight.",
      body: "Discophile is getting ready for its first App Store release, for iPhone with iOS 17 or later, in English and Italian. Until then, request an invitation to the beta. Bring your own server: it works with the music and audiobooks you already host.",
      servers: "Supported servers",
      serverNames: ["Navidrome", "Subsonic and OpenSubsonic", "Jellyfin", "Audiobookshelf", "Internet radio"],
      question: "Have a question or an idea? Write to",
    },
    sponsor: {
      title: "Help Discophile grow.",
      body: "If Discophile is useful to you, you can support me on GitHub Sponsors. Even a small contribution helps me spend more time on fixes, accessibility and new features.",
      button: "Support on GitHub Sponsors",
    },
    footerNote: "Screenshots show a demo library of fictional artists and albums. © 2026 Andrea Debernardi",
  },
  it: {
    title: "La tua musica. Il tuo mondo. Premi play.",
    lede: "La tua libreria merita il palco. Porta su iPhone la musica di Navidrome, Subsonic e Jellyfin, ritrova i tuoi audiolibri di Audiobookshelf e accendi le radio che ami. Tutto in Discophile, senza creare un altro account e senza tracciamento.",
    soon: "Presto sull'App Store",
    fineprint: "Per iPhone con iOS 17 o versioni successive, in italiano e in inglese.",
    listen: {
      title: "Riscopri i tuoi pezzi. Trova la prossima ossessione.",
      body: [
        <>Apri Ascolta e lasciati ispirare da mix basati sui tuoi ascolti su questo iPhone. <b>Da riascoltare</b> ti ripropone i brani che non ascolti da un mese, <b>Ancora da scoprire</b> trova quelli che non hai mai ascoltato e <b>Hai 25 minuti?</b> dà la colonna sonora alla tua pausa.</>,
        <><b>Scegli un disco per stasera</b> ti propone tre album interi da 25, 45 o 60 minuti, poi riproduce quello che scegli dalla prima all&apos;ultima traccia.</>,
      ],
      screens: [
        ["listen-now", "La schermata Ascolta con i mix di scoperta e gli album suggeriti"],
        ["album", "La pagina di un album con Riproduci, Casuale e l'elenco delle tracce"],
        ["now-playing", "La schermata In riproduzione, con copertina grande, barra di avanzamento e controlli"],
      ],
    },
    library: {
      title: "Tutta la tua collezione. Sempre con te.",
      body: <>Sfoglia album, artisti, brani e playlist. Trova quello che cerchi in un solo posto e scarica i tuoi preferiti per portarli anche dove la rete non arriva. Attiva <b>Solo scaricati</b> per vedere in tutta l’app solo i contenuti salvati sul telefono.</>,
      region: "Schermate della libreria, scorrevoli",
      screens: [
        ["library-albums", "La griglia degli album nella libreria", "Album"],
        ["library-artists", "L'elenco degli artisti nella libreria", "Artisti"],
        ["search", "Risultati di ricerca per artisti, album e brani", "Cerca"],
        ["statistics", "Statistiche di ascolto degli ultimi 30 giorni", "Statistiche di ascolto"],
        ["statistics-charts", "Minuti ascoltati al giorno e brani più ascoltati", "Brani più ascoltati"],
      ],
    },
    assistant: {
      title: "Tu dai l’idea. La tua musica fa il resto.",
      body: [
        "Descrivi l’atmosfera, un’occasione o quello che ti va di ascoltare. L’assistente sceglie i brani dalla tua libreria e li mette subito a portata di play. Puoi usare Apple Intelligence direttamente sull’iPhone, senza inviare dati all’esterno, oppure collegare il tuo account ChatGPT.",
        <>Vuoi togliere brani o rinominare una playlist? Prima ti viene mostrato un riepilogo. Sul tuo server non cambia nulla finché non tocchi <b>Conferma modifiche</b>.</>,
      ],
      facts: [
        <><b>Su questo iPhone</b> usa il modello locale di Apple su iOS 26 con Apple Intelligence.</>,
        <><b>ChatGPT</b> riceve la tua richiesta e i metadati dei brani, mai audio, password o cronologia di ascolto.</>,
        <><b>Chiedi al tuo curatore</b> ti prepara mezz&apos;ora di musica per una serata tranquilla o per concentrarti.</>,
      ],
      screens: [
        ["ai-local", "Una playlist creata sull'iPhone, con Riproduci e Crea playlist"],
        ["ai-chatgpt", "ChatGPT spiega perché una canzone è orecchiabile e sceglie brani simili"],
        ["curator", "La schermata del curatore con motore AI, modello e occasione di ascolto"],
      ],
    },
    audiobooks: {
      title: "Un altro capitolo. Poi un altro ancora.",
      body: "Porta con te le storie di Audiobookshelf, insieme alla musica o anche da sole. Discophile salva il punto di ascolto anche offline e, quando sei online, lo sincronizza con il server ogni pochi secondi. Passa dall’iPhone al web e riprendi da dove eri rimasto.",
      facts: [
        "Velocità di riproduzione da 0,5x a 4x, capitoli sulla barra di avanzamento e salti precisi nel punto che vuoi.",
        "Timer di spegnimento, anche a fine capitolo.",
        "Segnalibri e libri interi da scaricare per ascoltarli anche offline.",
      ],
      screens: [
        ["audiobook", "La pagina di un audiolibro con Continua ad ascoltare e i capitoli"],
        ["audiobook-player", "Il player degli audiolibri sul capitolo 1, con i pulsanti per andare avanti e indietro e la velocità"],
      ],
    },
    details: {
      title: "Il bello si sente anche nei dettagli.",
      items: [
        ["Senza pause e dissolvenza", "Gli album si ascoltano senza interruzioni tra un brano e l'altro. Dissolvenza incrociata da 1 a 12 secondi, che dentro un album lascia il posto alla riproduzione senza pause."],
        ["Equalizzatore", "Dieci bande, oppure la correzione per le tue cuffie con AutoEq. Ci sono anche ReplayGain e un limite di volume."],
        ["Testi", "Segui le parole a tempo con la musica. Tocca una strofa per riascoltarla subito."],
        ["Podcast e radio", "Aggiungi i podcast con un link di Apple Podcasts o Spotify e salva le tue radio online preferite."],
        ["Modalità sport", "Impostazioni pronte per corsa, camminata, bici e palestra, da avviare con Comandi Rapidi."],
        ["Siri e schermata di blocco", "Chiedi un album a Siri e controlla la riproduzione dalla schermata di blocco, dalle cuffie o con AirPlay."],
      ],
    },
    privacy: {
      label: "Privacy",
      text: "Discophile non ha un server. Le password restano nel portachiavi di iOS, la cronologia di ascolto resta sul tuo iPhone e nell'app non ci sono strumenti di analisi.",
    },
    release: {
      title: "Il prossimo ascolto comincia qui.",
      body: "Discophile sta per arrivare sull’App Store. Vuoi provarlo in anteprima? Richiedi un invito alla beta su TestFlight e porta la tua libreria. Ti serve un iPhone con iOS 17 o versioni successive e un server compatibile. L’app è disponibile in italiano e in inglese.",
      servers: "Server supportati",
      serverNames: ["Navidrome", "Subsonic e OpenSubsonic", "Jellyfin", "Audiobookshelf", "Radio online"],
      question: "Hai una domanda o un'idea? Scrivi a",
    },
    sponsor: {
      title: "Dai una spinta al prossimo giro.",
      body: "Se Discophile ti è utile, puoi sostenermi su GitHub Sponsors. Anche un piccolo contributo mi permette di dedicare più tempo a correzioni, accessibilità e nuove funzioni.",
      button: "Sostieni su GitHub Sponsors",
    },
    footerNote: "Le schermate mostrano l'app in inglese, con una libreria dimostrativa di artisti e album di fantasia. © 2026 Andrea Debernardi",
  },
};

function Screen({ name, alt, caption }: { name: string; alt: string; caption?: string }) {
  return (
    <figure className="phone">
      <img src={`/assets/screens/${name}.webp`} alt={alt} width={645} height={1398} loading="lazy" />
      {caption && <figcaption>{caption}</figcaption>}
    </figure>
  );
}

export function HomePage({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  return (
    <>
      <main id="main" tabIndex={-1}>
        <section className="hero">
          <div className="hero-copy">
            <h1>{c.title}</h1>
            <p className="lede">{c.lede}</p>
            <div className="actions">
              <TestFlightButton />
              <HeroVideo locale={locale} />
            </div>
            <p className="fineprint soon">
              <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="2" /><path d="M12 7v5l3 2" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>
              <span><b>{c.soon}.</b> {c.fineprint}</span>
            </p>
          </div>
          <div className="turntable" aria-hidden="true">
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
            <figure className="phone hero-phone">
              <img src="/assets/screens/now-playing.webp" alt="" width={645} height={1398} fetchPriority="high" />
            </figure>
          </div>
        </section>

        <section className="feature" id="listen" aria-labelledby="listen-title">
          <div className="feature-copy">
            <h2 id="listen-title">{c.listen.title}</h2>
            {c.listen.body.map((text, i) => <p key={i}>{text}</p>)}
          </div>
          <div className="phones trio">
            {c.listen.screens.map(([name, alt]) => <Screen key={name} name={name} alt={alt} />)}
          </div>
        </section>

        <section className="shelf-section" id="library" aria-labelledby="library-title">
          <div className="feature-copy narrow">
            <h2 id="library-title">{c.library.title}</h2>
            <p>{c.library.body}</p>
          </div>
          <div className="shelf" tabIndex={0} role="region" aria-label={c.library.region}>
            {c.library.screens.map(([name, alt, caption]) => <Screen key={name} name={name} alt={alt} caption={caption} />)}
          </div>
        </section>

        <section className="feature assistant" id="assistant" aria-labelledby="assistant-title">
          <div className="feature-copy">
            <h2 id="assistant-title">{c.assistant.title}</h2>
            {c.assistant.body.map((text, i) => <p key={i}>{text}</p>)}
            <ul className="facts">
              {c.assistant.facts.map((fact, i) => <li key={i}>{fact}</li>)}
            </ul>
          </div>
          <div className="phones fan">
            {c.assistant.screens.map(([name, alt]) => <Screen key={name} name={name} alt={alt} />)}
          </div>
        </section>

        <section className="feature reverse" id="audiobooks" aria-labelledby="audiobooks-title">
          <div className="feature-copy">
            <h2 id="audiobooks-title">{c.audiobooks.title}</h2>
            <p>{c.audiobooks.body}</p>
            <ul className="facts">
              {c.audiobooks.facts.map((fact) => <li key={fact}>{fact}</li>)}
            </ul>
          </div>
          <div className="phones pair">
            {c.audiobooks.screens.map(([name, alt]) => <Screen key={name} name={name} alt={alt} />)}
          </div>
        </section>

        <section className="details" aria-labelledby="details-title">
          <h2 id="details-title">{c.details.title}</h2>
          <dl>
            {c.details.items.map(([term, text]) => <div key={term}><dt>{term}</dt><dd>{text}</dd></div>)}
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
          <TestFlightButton />
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

      <SiteFooter locale={locale} note={c.footerNote} />
    </>
  );
}
