import type { ReactNode } from "react";
import { Icon, type IconName } from "@/components/Icon";
import { SiteFooter } from "@/components/SiteFooter";
import { DeviceDock, DeviceShots, type PhoneLayout, type Shot } from "@/components/Shots";
import { localePath, ui, type Locale } from "@/lib/i18n";

type Feature = {
  id: string;
  title: string;
  body: ReactNode[];
  facts?: ReactNode[];
  iphone?: Shot[];
  desktop?: Shot[];
  layout?: PhoneLayout;
  style?: "reverse" | "dark";
};

type Copy = {
  title: string;
  lede: ReactNode;
  features: Feature[];
  more: { title: string; items: [string, string][] };
  next: { title: string; body: string; devices: string; selfHosting: string };
};

const COPY: Record<Locale, Copy> = {
  en: {
    title: "Everything Discophile does.",
    lede: "One app on iPhone and on the desktop, with the same mixes, library, assistant and audiobooks. Use the switch above each set of screenshots to see it on either.",
    features: [
      {
        id: "listen",
        title: "Something familiar. Something unexpected.",
        body: [
          <>Listen Now opens with mixes built from your own listening on that device. <b>Worth another listen</b> brings back songs you haven&apos;t played in a month, <b>Still to discover</b> finds the ones you never played, and <b>Have 25 minutes?</b> fits a short break.</>,
          <><b>Choose a record for tonight</b> suggests three whole albums for 25, 45 or 60 minutes, then plays the one you pick from first track to last. <b>Pick up where you left off</b> brings back your queue, even after a restart.</>,
        ],
        iphone: [["listen-now", "Listen Now with discovery mixes and album suggestions"], ["album", "An album page with Play, Shuffle and the track list"], ["now-playing", "Now Playing with large artwork and transport controls"]],
        desktop: [["listen-now", "Listen Now on the desktop, with Pick up where you left off, mixes and album suggestions"]],
      },
      {
        id: "library",
        title: "The whole shelf, within reach.",
        body: [
          <>Browse albums, artists, songs, playlists and favorites, and search everything from one field. Create and edit playlists without duplicates, and switch between several libraries, each with its own name, history and downloads.</>,
          <>Download songs, albums, playlists and podcasts for offline listening, and switch on <b>Downloaded only</b> to see just what is saved on the device.</>,
        ],
        iphone: [["library-albums", "The library album grid", "Albums"], ["library-artists", "The library artist list", "Artists"], ["search", "Search results for artists, albums and songs", "Search"]],
        desktop: [["albums", "The album grid in a desktop window"], ["album", "An album page with Play, Shuffle and the track list"]],
        style: "reverse",
      },
      {
        id: "now-playing",
        title: "Lyrics that keep time.",
        body: [
          "Now Playing takes on the colors of the artwork. Synced lyrics follow the song and jump to a line when you tap or click it. They come from your server, with optional LRCLIB lookups when it has none.",
          <>Shuffle, repeat and a sleep timer are one step away, and <b>personal radio</b> keeps going after the last song: stay with the same style, or let it explore, with &quot;Why this song?&quot; for every pick.</>,
        ],
        facts: [
          "Gapless albums, and crossfade from 1 to 12 seconds that stays gapless inside an album.",
          "Lock Screen, Control Center and AirPlay on iPhone; media keys and the system controls on the desktop.",
        ],
        iphone: [["now-playing", "Now Playing on iPhone with the Discophile artwork"]],
        desktop: [["now-playing", "Now Playing on the desktop, with synced lyrics beside the artwork"]],
        layout: "single",
      },
      {
        id: "assistant",
        title: "Ask for a playlist. Get songs you own.",
        body: [
          "The music assistant turns a sentence into a selection from your library, so every song it suggests is one you can play right now. Use Apple Intelligence on the iPhone, a local model such as Ollama on the desktop, or connect your ChatGPT account.",
          <>Edits such as removing tracks or renaming a playlist open a review first. Nothing changes on your server until you confirm. <b>About</b> writes an overview of an artist, album, song or podcast from your server&apos;s notes and, if you allow it, MusicBrainz and Wikipedia.</>,
        ],
        facts: [
          <><b>ChatGPT</b> receives your request and song metadata, never audio, passwords or listening history.</>,
          <><b>Agents</b> such as Claude can use your library from the desktop app, only on your computer and only with your OK.</>,
        ],
        iphone: [["ai-local", "A playlist for a rainy Sunday generated on the iPhone, with Play and Review playlist"], ["ai-chatgpt", "Apple Intelligence on the iPhone explaining why Discophile by Mina Sol is catchy"], ["ai-chatgpt-songs", "A selection of songs like Discophile, picked on the iPhone from the library"]],
        desktop: [["assistant", "The music assistant beside Listen Now, proposing a 17-song playlist"]],
        layout: "fan",
        style: "dark",
      },
      {
        id: "statistics",
        title: "Your listening, in numbers.",
        body: [
          "Time listened, plays, songs, artists, active days and your current streak, for the last 7 days, 30 days, 12 months or all time. Top songs, artists, albums and genres show what you really come back to.",
          "Statistics are computed on the device from your own history, separately for each library. Nothing is sent anywhere.",
        ],
        iphone: [["statistics", "Listening statistics for the last 30 days"], ["statistics-charts", "Minutes listened per day and top songs"]],
        desktop: [["statistics", "Listening statistics on the desktop for the last 30 days"]],
        layout: "pair",
        style: "reverse",
      },
      {
        id: "audiobooks",
        title: "Audiobooks pick up where you stopped.",
        body: ["Connect Audiobookshelf next to your music, or on its own. Positions sync with the server every few seconds and keep working offline, so you can move between your phone, your computer and the web without hunting for your place."],
        facts: [
          "Speed from 0.5x to 4x, chapter and book timelines, and 30-second skips.",
          "Sleep timer, including at the end of the chapter.",
          "Bookmarks, and whole books downloaded for offline listening.",
        ],
        iphone: [["audiobook", "An audiobook page with Continue listening and chapters"], ["audiobook-player", "The audiobook player on chapter 13, with skip buttons and speed"]],
        desktop: [["audiobooks", "The audiobook library on the desktop"], ["audiobook-player", "An audiobook playing on chapter 13 of 34"]],
        layout: "pair",
      },
      {
        id: "sound",
        title: "Tuned to your ears.",
        body: [
          "A ten-band equalizer with presets, or a correction for your headphones from AutoEq, with ParametricEQ import. On the desktop, the response curve shows exactly what changes.",
          "ReplayGain in track, album or automatic mode keeps the volume even, with clipping prevention and an optional volume limit.",
        ],
        desktop: [["equalizer", "The equalizer with the Loudness preset and its response curve"]],
        style: "reverse",
      },
    ],
    more: {
      title: "And the rest of the collection.",
      items: [
        ["Podcasts and radio", "Subscribe with an Apple Podcasts or Spotify link or an RSS feed, and save internet radio stations without an account."],
        ["Offline listening", "Download what you love, and keep mixes and chosen playlists downloaded automatically within a space limit."],
        ["Several libraries", "Navidrome, Subsonic, OpenSubsonic and Jellyfin side by side, each with its own name, cache and downloads."],
        ["Scrobbling", "Send your listening history to your server, if you want it there too."],
        ["Accessible", "Labels for VoiceOver and screen readers, keyboard control on the desktop, and text that scales."],
        ["English and Italian", "Discophile follows your system language, or the one you choose."],
      ],
    },
    next: {
      title: "Where will you listen?",
      body: "Discophile runs on iPhone, Mac, Windows and Linux, and in any browser from a server you host.",
      devices: "Download Discophile",
      selfHosting: "Host the web player",
    },
  },
  it: {
    title: "Tutto quello che fa Discophile.",
    lede: "Un'unica app su iPhone e sul computer, con gli stessi mix, la stessa libreria, lo stesso assistente e gli stessi audiolibri. Usa il selettore sopra ogni gruppo di schermate per vederli su entrambi.",
    features: [
      {
        id: "listen",
        title: "Riscopri i tuoi pezzi. Trova la prossima ossessione.",
        body: [
          <>Apri Ascolta e lasciati ispirare da mix basati sui tuoi ascolti su quel dispositivo. <b>Da riascoltare</b> ti ripropone i brani che non ascolti da un mese, <b>Ancora da scoprire</b> trova quelli che non hai mai ascoltato e <b>Hai 25 minuti?</b> dà la colonna sonora alla tua pausa.</>,
          <><b>Scegli un disco per stasera</b> ti propone tre album interi da 25, 45 o 60 minuti, poi riproduce quello che scegli dalla prima all&apos;ultima traccia. <b>Riprendi da dove eri rimasto</b> ti restituisce la coda, anche dopo un riavvio.</>,
        ],
        iphone: [["listen-now", "La schermata Ascolta con i mix di scoperta e gli album suggeriti"], ["album", "La pagina di un album con Riproduci, Casuale e l'elenco delle tracce"], ["now-playing", "In riproduzione, con copertina grande e controlli"]],
        desktop: [["listen-now", "Ascolta sul desktop, con Riprendi da dove eri rimasto, i mix e gli album suggeriti"]],
      },
      {
        id: "library",
        title: "Tutta la tua collezione, a portata di mano.",
        body: [
          <>Sfoglia album, artisti, brani, playlist e preferiti, e cerca tutto da un solo campo. Crea e modifica playlist senza doppioni, e passa da una libreria all&apos;altra, ognuna con il suo nome, la sua cronologia e i suoi download.</>,
          <>Scarica brani, album, playlist e podcast per ascoltarli offline, e attiva <b>Solo scaricati</b> per vedere solo ciò che è salvato sul dispositivo.</>,
        ],
        iphone: [["library-albums", "La griglia degli album nella libreria", "Album"], ["library-artists", "L'elenco degli artisti nella libreria", "Artisti"], ["search", "Risultati di ricerca per artisti, album e brani", "Cerca"]],
        desktop: [["albums", "La griglia degli album in una finestra desktop"], ["album", "La pagina di un album con Riproduci, Casuale e l'elenco delle tracce"]],
        style: "reverse",
      },
      {
        id: "now-playing",
        title: "Testi che vanno a tempo.",
        body: [
          "In riproduzione prende i colori della copertina. I testi sincronizzati seguono il brano e saltano alla riga che tocchi o clicchi. Arrivano dal tuo server e, se manca, da LRCLIB, solo se lo attivi.",
          <>Casuale, ripeti e timer di spegnimento sono a un passo, e la <b>radio personale</b> continua dopo l&apos;ultimo brano: resta sullo stesso stile oppure esplora, con &quot;Perché questo brano?&quot; per ogni scelta.</>,
        ],
        facts: [
          "Album senza pause, e dissolvenza incrociata da 1 a 12 secondi che dentro un album lascia il posto alla riproduzione continua.",
          "Schermata di blocco, Centro di Controllo e AirPlay su iPhone; tasti multimediali e controlli di sistema sul desktop.",
        ],
        iphone: [["now-playing", "In riproduzione su iPhone con la copertina di Discophile"]],
        desktop: [["now-playing", "In riproduzione sul desktop, con i testi sincronizzati accanto alla copertina"]],
        layout: "single",
      },
      {
        id: "assistant",
        title: "Tu dai l’idea. La tua musica fa il resto.",
        body: [
          "Descrivi l’atmosfera, un’occasione o quello che ti va di ascoltare. L’assistente sceglie i brani dalla tua libreria e li mette subito a portata di play. Usa Apple Intelligence sull’iPhone, un modello locale come Ollama sul desktop, oppure collega il tuo account ChatGPT.",
          <>Vuoi togliere brani o rinominare una playlist? Prima ti viene mostrato un riepilogo, e sul tuo server non cambia nulla finché non confermi. <b>Informazioni</b> scrive una panoramica di un artista, un album, un brano o un podcast a partire dalle note del tuo server e, se lo consenti, da MusicBrainz e Wikipedia.</>,
        ],
        facts: [
          <><b>ChatGPT</b> riceve la tua richiesta e i metadati dei brani, mai audio, password o cronologia di ascolto.</>,
          <><b>Gli agenti</b> come Claude possono usare la tua libreria dall&apos;app desktop, solo sul tuo computer e solo con il tuo permesso.</>,
        ],
        iphone: [["ai-local", "Una playlist per una domenica di pioggia creata sull'iPhone, con Riproduci e Rivedi playlist"], ["ai-chatgpt", "Apple Intelligence sull'iPhone spiega perché Discophile di Mina Sol è orecchiabile"], ["ai-chatgpt-songs", "Una selezione di brani simili a Discophile, scelti sull'iPhone dalla libreria"]],
        desktop: [["assistant", "L'assistente musicale accanto ad Ascolta, che propone una playlist di 17 brani"]],
        layout: "fan",
        style: "dark",
      },
      {
        id: "statistics",
        title: "I tuoi ascolti, in numeri.",
        body: [
          "Tempo di ascolto, riproduzioni, brani, artisti, giorni attivi e la serie in corso, per gli ultimi 7 giorni, 30 giorni, 12 mesi o da sempre. Brani, artisti, album e generi più ascoltati mostrano a cosa torni davvero.",
          "Le statistiche sono calcolate sul dispositivo dalla tua cronologia, separatamente per ogni libreria. Non viene inviato nulla.",
        ],
        iphone: [["statistics", "Statistiche di ascolto degli ultimi 30 giorni"], ["statistics-charts", "Minuti ascoltati al giorno e brani più ascoltati"]],
        desktop: [["statistics", "Statistiche di ascolto sul desktop per gli ultimi 30 giorni"]],
        layout: "pair",
        style: "reverse",
      },
      {
        id: "audiobooks",
        title: "Un altro capitolo. Poi un altro ancora.",
        body: ["Porta con te le storie di Audiobookshelf, insieme alla musica o anche da sole. Discophile salva il punto di ascolto anche offline e lo sincronizza con il server ogni pochi secondi. Passa dall’iPhone al computer o al web e riprendi da dove eri rimasto."],
        facts: [
          "Velocità da 0,5x a 4x, capitoli e libro sulla barra di avanzamento, e salti di 30 secondi.",
          "Timer di spegnimento, anche a fine capitolo.",
          "Segnalibri e libri interi da scaricare per ascoltarli anche offline.",
        ],
        iphone: [["audiobook", "La pagina di un audiolibro con Continua ad ascoltare e i capitoli"], ["audiobook-player", "Il player degli audiolibri al capitolo 13, con i pulsanti per saltare e la velocità"]],
        desktop: [["audiobooks", "La libreria degli audiolibri sul desktop"], ["audiobook-player", "Un audiolibro in riproduzione al capitolo 13 di 34"]],
        layout: "pair",
      },
      {
        id: "sound",
        title: "Accordato sulle tue orecchie.",
        body: [
          "Un equalizzatore a dieci bande con preset, oppure la correzione per le tue cuffie con AutoEq, con importazione ParametricEQ. Sul desktop la curva di risposta mostra esattamente cosa cambia.",
          "ReplayGain in modalità brano, album o automatica mantiene il volume uniforme, con protezione dalla distorsione e un limite di volume facoltativo.",
        ],
        desktop: [["equalizer", "L'equalizzatore con il preset Loudness e la sua curva di risposta"]],
        style: "reverse",
      },
    ],
    more: {
      title: "E il resto della collezione.",
      items: [
        ["Podcast e radio", "Aggiungi i podcast con un link di Apple Podcasts o Spotify o un feed RSS, e salva le radio online senza account."],
        ["Ascolto offline", "Scarica ciò che ami, e tieni scaricati in automatico mix e playlist scelte entro un limite di spazio."],
        ["Più librerie", "Navidrome, Subsonic, OpenSubsonic e Jellyfin fianco a fianco, ognuna con il suo nome, la sua cache e i suoi download."],
        ["Scrobbling", "Invia la cronologia di ascolto al tuo server, se la vuoi anche lì."],
        ["Accessibile", "Etichette per VoiceOver e lettori di schermo, controllo da tastiera sul desktop e testo ridimensionabile."],
        ["Italiano e inglese", "Discophile segue la lingua del sistema, oppure quella che scegli tu."],
      ],
    },
    next: {
      title: "Dove vuoi ascoltare?",
      body: "Discophile funziona su iPhone, Mac, Windows e Linux, e in qualsiasi browser da un server che ospiti tu.",
      devices: "Scarica Discophile",
      selfHosting: "Ospita il player web",
    },
  },
};

const MORE_ICONS: IconName[] = ["radio", "download", "libraries", "scrobble", "accessible", "language"];

export function FeaturesPage({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  const labels = ui[locale].shots;
  return (
    <>
      <main id="main" tabIndex={-1}>
        <header className="page-head">
          <h1>{c.title}</h1>
          <p className="lede">{c.lede}</p>
        </header>

        <DeviceDock labels={labels} />

        {c.features.map((feature) => (
          <section
            key={feature.id}
            id={feature.id}
            className={`feature${feature.style === "reverse" ? " reverse" : ""}${feature.style === "dark" ? " assistant" : ""}`}
            aria-labelledby={`${feature.id}-title`}
          >
            <div className="feature-copy">
              <h2 id={`${feature.id}-title`}>{feature.title}</h2>
              {feature.body.map((text, i) => <p key={i}>{text}</p>)}
              {feature.facts && (
                <ul className="facts">
                  {feature.facts.map((fact, i) => <li key={i}>{fact}</li>)}
                </ul>
              )}
            </div>
            <DeviceShots
              labels={{ ...labels, group: `${labels.group} ${feature.title}` }}
              iphone={feature.iphone}
              desktop={feature.desktop}
              layout={feature.layout}
            />
          </section>
        ))}

        <section className="details" aria-labelledby="more-title">
          <h2 id="more-title">{c.more.title}</h2>
          <dl>
            {c.more.items.map(([term, text], i) => <div key={term}><dt><Icon name={MORE_ICONS[i]} />{term}</dt><dd>{text}</dd></div>)}
          </dl>
        </section>

        <section className="next-steps" aria-labelledby="next-title">
          <h2 id="next-title">{c.next.title}</h2>
          <p>{c.next.body}</p>
          <div className="actions">
            <a className="button primary" href={localePath(locale, "/download")}>{c.next.devices}</a>
            <a className="button" href={localePath(locale, "/self-hosting")}>{c.next.selfHosting}</a>
          </div>
        </section>
      </main>
      <SiteFooter locale={locale} note={ui[locale].screenshotsNote} />
    </>
  );
}
