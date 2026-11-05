export const LOCALES = ["en", "it"] as const;
export type Locale = (typeof LOCALES)[number];

export function isLocale(value: unknown): value is Locale {
  return LOCALES.includes(value as Locale);
}

/** Remembers the language picked in the switcher, so it beats the browser's preference. */
export const LOCALE_COOKIE = "discophile-locale";

/** The first supported language in an Accept-Language header, by quality. */
export function preferredLocale(header: string | null): Locale {
  const ranked = (header ?? "")
    .split(",")
    .map((part) => {
      const [tag, ...params] = part.trim().toLowerCase().split(";");
      const q = params.find((param) => param.trim().startsWith("q="));
      return { language: tag.split("-")[0], q: q ? Number(q.trim().slice(2)) || 0 : 1 };
    })
    .filter((entry) => entry.q > 0)
    .sort((a, b) => b.q - a.q);
  return ranked.find((entry) => isLocale(entry.language))?.language as Locale | undefined ?? "en";
}

/** Path of a page in a locale: English lives at the root, Italian under /it. */
export function localePath(locale: Locale, path: string) {
  if (locale === "en") return path;
  return path === "/" ? "/it" : `/it${path}`;
}

/** Strips the locale prefix, so "/it/privacy" becomes "/privacy". */
export function basePath(pathname: string) {
  if (pathname === "/it") return "/";
  return pathname.startsWith("/it/") ? pathname.slice(3) : pathname;
}

export function alternates(path: string) {
  return {
    canonical: path,
    languages: { en: path, it: localePath("it", path), "x-default": path },
  };
}

export const ui = {
  en: {
    skip: "Skip to content",
    menu: "Menu",
    mainNav: "Main",
    nav: { features: "Features", download: "Download", selfHosting: "Self-hosting", faq: "FAQ" },
    shots: { group: "Screenshots for", switch: "Show screenshots for", iphone: "Mobile", desktop: "Desktop", mobileNote: "Mobile screenshots show the iPhone app. Discophile is also available on Android." },
    theme: { label: "Appearance", auto: "Automatic", light: "Light", dark: "Dark" },
    newsletter: {
      started: "Your download is starting.",
      startedBody: "If it doesn't start,",
      directLink: "download it directly",
      sponsorTitle: "Support Discophile",
      sponsorBody: "Discophile is free and open source. If it's useful to you, a sponsorship on GitHub helps me spend more time on fixes, accessibility and new features.",
      sponsorButton: "Become a sponsor",
      title: "Get the newsletter",
      body: "New releases and what's coming next, a few times a year. No spam.",
      email: "Email",
      consentBefore: "I have read and accept the",
      consentLink: "privacy policy",
      submit: "Subscribe",
      sending: "Subscribing…",
      done: "Almost there: check your inbox and confirm your subscription.",
      invalid: "Enter an email address in the format name@example.com.",
      consentRequired: "Accept the privacy policy to subscribe.",
      failed: "The subscription didn't go through. Try again later.",
      back: "Back to downloads",
    },
    download: {
      title: "Download Discophile",
      close: "Close",
      platforms: "Platform",
      names: { mac: "Mac", windows: "Windows", linux: "Linux", android: "Android", ios: "iPhone" },
      buttons: { mac: "Download for Mac", windows: "Download for Windows", linux: "Download for Linux", android: "Download APK", ios: "Download on the App Store" },
      version: "Version",
      notes: "Release notes",
      newTab: "(opens in a new tab)",
      file: "Download",
      allPlatforms: "All downloads and installation guides",
      copy: "Copy",
      copied: "Copied",
      options: {
        mac: [["Universal disk image", "Apple silicon and Intel, macOS 11 or later"]],
        windows: [["Portable zip", "64-bit, Windows 10 or later"]],
        linux: [["Tarball", "x86_64, with a .desktop file and icon"]],
        android: [["Android APK", "Signed release · Android 8 or later"]],
        ios: [["Discophile for iPhone", "App Store · €2.99 · iOS 17 or later"]],
      },
      linuxPackages: {
        deb: ["Debian / Ubuntu", "x86_64 · install with sudo apt install ./discophile_VERSION_amd64.deb"],
        appimage: ["AppImage", "x86_64 · make executable and launch; no installation"],
      },
      aur: ["AUR", "Arch Linux and derivatives"],
      tips: {
        mac: "Signed and notarized by Apple. Open the disk image and drag Discophile to Applications.",
        windows: "Extract the zip and run Discophile.exe. Windows may ask you to confirm the first launch.",
        linux: "Ubuntu 22.04 or equivalent, glibc 2.35+. AppImage: chmod +x, then launch (without FUSE: --appimage-extract-and-run). Tarball: extract and run discophile. Audio through PipeWire, PulseAudio or ALSA.",
        android: "Install this signed APK on Android 8 or later. Read the installation guide before updating an existing app.",
        ios: "A paid app on the App Store, with updates installed by your iPhone. The price may differ outside the euro area.",
      },
      unavailable: "This file isn't in the latest release yet. Check the release notes on GitHub; no download is available here until the file is published.",
      web: "In a browser?",
      webLink: "Host the web player",
    },
    mobile: {
      unavailable: "The mobile file is not in the latest published release yet.",
      android: {
        summary: "How to install the APK on Android",
        intro: "The APK is a signed Android app package. You do not need Google Play to install it.",
        steps: [
          "Download the APK on your Android 8 or later device and open it from Downloads.",
          "If Android asks, allow installation from the browser or file manager you used, then confirm the installation. You can turn that permission off afterwards; do not disable Play Protect.",
          "For updates, install the new APK over the existing app. Android requires the same signing key; development/debug builds may have a different key.",
        ],
        warning: "If Android reports a signing conflict, do not uninstall or clear app data to bypass it: that can erase accounts, downloads and listening progress. Keep the existing app until you have a backup or migration plan.",
      },
    },
    screenshotsNote: "Screenshots show a demo library of fictional artists and albums. © 2026 Andrea Debernardi",
    language: "Language",
    footer: {
      madeBy: "Made with care by",
      nav: "Footer",
      privacy: "Privacy policy",
      cookies: "Cookie policy",
      cookiePreferences: "Cookie preferences",
      sponsor: "Sponsor Discophile",
      github: "Source code on GitHub",
      otherProjects: "Other projects",
      newTab: "(opens in a new tab)",
      products: {
        gitdeck: "GitHub, GitLab and Forgejo on iPhone and iPad",
        tabularis: "Open-source desktop SQL workspace",
        melicum: "HDMI and DisplayPort volume control for macOS",
      },
    },
    errors: {
      unreadable: "The request could not be read. Try again.",
      rateLimited: "Too many requests. Wait a few minutes and try again.",
    },
    cookies: {
      title: "Cookies on this site",
      text: "This site counts visits with Matomo, hosted by the developer, without cookies. With your permission it can also set measurement cookies to recognise returning visitors. Nothing is shared with third parties.",
      link: "Read the cookie policy",
      current: "Current choice:",
      allowed: "measurement cookies allowed",
      cookieless: "cookieless only",
      allow: "Allow measurement cookies",
      deny: "Cookieless only",
    },
    video: {
      watch: "Watch the video",
      permalink: "Open video page",
      home: "Discover Discophile",
      title: "Drop the needle.",
      duration: "58 seconds",
      region: "Discophile intro video",
      close: "Close video",
      download: "Download the Discophile intro video (MP4, 12 MB)",
      captions: "English",
      note: "A 58-second tour of the app, set to the song Discophile by Mina Sol. Captions are available. The singer and the library are fictional.",
      transcript: "Video description and lyrics",
      shows: "What the video shows",
      lyrics: "Lyrics",
      scenes: [
        "The Listen Now screen, with discovery mixes and a song playing.",
        "Title cards: Your music. Your audiobooks. Your AI.",
        "Works with Navidrome, Subsonic, Jellyfin and Audiobookshelf. The library opens on its list of artists, then an album plays.",
        "Synced lyrics follow the song. Title card: Every album. Every track.",
        "Title card: Now, just ask. A playlist is generated on the iPhone, then the curator suggests a listening occasion.",
        "Title card: Or connect ChatGPT. A request is typed, ChatGPT answers and picks songs from the library.",
        "Title card: It checks with you first. A review lists the changes before they reach the server.",
        "Title cards: And your audiobooks. Pick up where you left off. An audiobook resumes in its player.",
        "Closing: the Discophile icon and the line Your music. Your audiobooks. Just ask.",
      ],
    },
  },
  it: {
    skip: "Vai al contenuto",
    menu: "Menu",
    mainNav: "Navigazione principale",
    nav: { features: "Funzionalità", download: "Download", selfHosting: "Self-hosting", faq: "FAQ" },
    shots: { group: "Schermate per", switch: "Mostra le schermate per", iphone: "Mobile", desktop: "Desktop", mobileNote: "Le schermate mobile mostrano l'app per iPhone. Discophile è disponibile anche per Android." },
    theme: { label: "Aspetto", auto: "Automatico", light: "Chiaro", dark: "Scuro" },
    newsletter: {
      started: "Il download sta per iniziare.",
      startedBody: "Se non parte,",
      directLink: "scaricalo direttamente",
      sponsorTitle: "Sostieni Discophile",
      sponsorBody: "Discophile è gratuito e open source. Se ti è utile, una sponsorizzazione su GitHub mi permette di dedicare più tempo a correzioni, accessibilità e nuove funzioni.",
      sponsorButton: "Diventa sponsor",
      title: "Iscriviti alla newsletter",
      body: "Nuove versioni e cosa sta arrivando, poche volte l'anno. Niente spam.",
      email: "Email",
      consentBefore: "Dichiaro di aver letto e accettato l'",
      consentLink: "informativa sulla privacy",
      submit: "Iscriviti",
      sending: "Iscrizione in corso…",
      done: "Ci siamo quasi: controlla la tua casella e conferma l'iscrizione.",
      invalid: "Inserisci un indirizzo email nel formato nome@esempio.it.",
      consentRequired: "Accetta l'informativa sulla privacy per iscriverti.",
      failed: "L'iscrizione non è andata a buon fine. Riprova più tardi.",
      back: "Torna ai download",
    },
    download: {
      title: "Scarica Discophile",
      close: "Chiudi",
      platforms: "Piattaforma",
      names: { mac: "Mac", windows: "Windows", linux: "Linux", android: "Android", ios: "iPhone" },
      buttons: { mac: "Scarica per Mac", windows: "Scarica per Windows", linux: "Scarica per Linux", android: "Scarica APK", ios: "Scarica su App Store" },
      version: "Versione",
      notes: "Note di rilascio",
      newTab: "(si apre in una nuova scheda)",
      file: "Scarica",
      allPlatforms: "Tutti i download e le guide di installazione",
      copy: "Copia",
      copied: "Copiato",
      options: {
        mac: [["Immagine disco universale", "Apple silicon e Intel, macOS 11 o successivi"]],
        windows: [["Zip portatile", "64 bit, Windows 10 o successivi"]],
        linux: [["Archivio tar", "x86_64, con file .desktop e icona"]],
        android: [["APK per Android", "Release firmata · Android 8 o versioni successive"]],
        ios: [["Discophile per iPhone", "App Store · 2,99 € · iOS 17 o versioni successive"]],
      },
      linuxPackages: {
        deb: ["Debian / Ubuntu", "x86_64 · installa con sudo apt install ./discophile_VERSION_amd64.deb"],
        appimage: ["AppImage", "x86_64 · rendi eseguibile e avvia; senza installazione"],
      },
      aur: ["AUR", "Arch Linux e derivate"],
      tips: {
        mac: "Firmato e autenticato da Apple. Apri l'immagine disco e trascina Discophile in Applicazioni.",
        windows: "Estrai lo zip e avvia Discophile.exe. Windows potrebbe chiederti di confermare il primo avvio.",
        linux: "Ubuntu 22.04 o equivalente, glibc 2.35+. AppImage: chmod +x, poi avvia (senza FUSE: --appimage-extract-and-run). Archivio tar: estrai e avvia discophile. Audio tramite PipeWire, PulseAudio o ALSA.",
        android: "Installa questo APK firmato su Android 8 o versioni successive. Leggi la guida prima di aggiornare un'app esistente.",
        ios: "App a pagamento su App Store, con gli aggiornamenti installati dal tuo iPhone. Fuori dall'area euro il prezzo può variare.",
      },
      unavailable: "Questo file non è ancora nell'ultima release. Controlla le note su GitHub: il download sarà disponibile qui solo dopo la pubblicazione del file.",
      web: "Nel browser?",
      webLink: "Ospita il player web",
    },
    mobile: {
      unavailable: "Il file per questo dispositivo non è ancora nell'ultima release pubblicata.",
      android: {
        summary: "Come installare l'APK su Android",
        intro: "L'APK è un pacchetto Android firmato. Per installarlo non serve Google Play.",
        steps: [
          "Scarica l'APK sul dispositivo con Android 8 o versioni successive e aprilo da Download.",
          "Se Android lo richiede, autorizza l'installazione dal browser o dal gestore di file utilizzato, poi conferma. Puoi revocare il permesso dopo l'installazione; non disattivare Play Protect.",
          "Per aggiornare, installa il nuovo APK sopra l'app esistente. Android richiede la stessa chiave di firma; le build di sviluppo/debug possono avere una chiave diversa.",
        ],
        warning: "Se Android segnala firme diverse, non disinstallare l'app e non cancellarne i dati per aggirare il problema: perderesti account, download e progressi di ascolto. Mantieni l'app esistente finché non hai un backup o un piano di migrazione.",
      },
    },
    screenshotsNote: "Le schermate mostrano l'app in inglese, con una libreria dimostrativa di artisti e album di fantasia. © 2026 Andrea Debernardi",
    language: "Lingua",
    footer: {
      madeBy: "Creato con cura da",
      nav: "Piè di pagina",
      privacy: "Informativa sulla privacy",
      cookies: "Cookie policy",
      cookiePreferences: "Preferenze cookie",
      sponsor: "Sostieni Discophile",
      github: "Codice sorgente su GitHub",
      otherProjects: "Altri progetti",
      newTab: "(si apre in una nuova scheda)",
      products: {
        gitdeck: "GitHub, GitLab e Forgejo su iPhone e iPad",
        tabularis: "Ambiente SQL open source per desktop",
        melicum: "Volume per schermi HDMI e DisplayPort su macOS",
      },
    },
    errors: {
      unreadable: "Impossibile leggere la richiesta. Riprova.",
      rateLimited: "Troppe richieste. Attendi qualche minuto e riprova.",
    },
    cookies: {
      title: "Cookie su questo sito",
      text: "Questo sito misura le visite con Matomo, installato sui server dello sviluppatore, senza usare cookie. Se lo consenti, può impostare anche cookie di misurazione per riconoscere chi torna a visitarlo. Nessun dato viene condiviso con terze parti.",
      link: "Leggi la cookie policy",
      current: "Scelta attuale:",
      allowed: "cookie di misurazione consentiti",
      cookieless: "nessun cookie di misurazione",
      allow: "Consenti cookie di misurazione",
      deny: "Continua senza cookie",
    },
    video: {
      watch: "Guarda il video",
      permalink: "Apri la pagina del video",
      home: "Scopri Discophile",
      title: "Alza il volume. Premi play.",
      duration: "58 secondi",
      region: "Video di presentazione di Discophile",
      close: "Chiudi il video",
      download: "Scarica il video di presentazione di Discophile (MP4, 12 MB)",
      captions: "Inglese (testo della canzone)",
      note: "Scopri Discophile in 58 secondi, sulle note di «Discophile» di Mina Sol. I sottotitoli riportano il testo della canzone, in inglese. La cantante e la libreria sono di fantasia.",
      transcript: "Descrizione del video e testo della canzone",
      shows: "Cosa mostra il video",
      lyrics: "Testo della canzone (in inglese)",
      scenes: [
        "La schermata Ascolta, con i mix di scoperta e una canzone in riproduzione.",
        "Scritte in sovrimpressione, in inglese: la tua musica, i tuoi audiolibri, la tua AI.",
        "Funziona con Navidrome, Subsonic, Jellyfin e Audiobookshelf. La libreria si apre sull'elenco degli artisti, poi parte un album.",
        "Il testo sincronizzato segue la canzone. Scritta: ogni album, ogni traccia.",
        "Scritta: ora basta chiedere. Sull'iPhone nasce una playlist, poi il curatore propone un'occasione di ascolto.",
        "Scritta: oppure collega ChatGPT. Si scrive una richiesta, ChatGPT risponde e sceglie i brani dalla libreria.",
        "Scritta: prima chiede a te. Un riepilogo mostra le modifiche prima che raggiungano il server.",
        "Scritte: e i tuoi audiolibri, riprendi da dove eri rimasto. Un audiolibro riparte nel suo player.",
        "In chiusura, l'icona di Discophile e la frase: la tua musica, i tuoi audiolibri, basta chiedere.",
      ],
    },
  },
};

export type UI = (typeof ui)["en"];
