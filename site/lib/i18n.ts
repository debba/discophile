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
    shots: { group: "Screenshots for", switch: "Show screenshots for", iphone: "iPhone", desktop: "Desktop" },
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
      names: { mac: "Mac", windows: "Windows", linux: "Linux" },
      version: "Version",
      notes: "Release notes",
      newTab: "(opens in a new tab)",
      file: "Download",
      copy: "Copy",
      copied: "Copied",
      options: {
        mac: [["Universal disk image", "Apple silicon and Intel, macOS 11 or later"]],
        windows: [["Portable zip", "64-bit, Windows 10 or later"]],
        linux: [["Tarball", "x86_64, with a .desktop file and icon"]],
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
      },
      unavailable: "This file isn't in the latest release yet. Every file is on GitHub.",
      iphone: "On iPhone?",
      iphoneLink: "Try the beta",
      web: "In a browser?",
      webLink: "Host the web player",
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
    testflight: {
      button: "Try in TestFlight",
      title: "Try Discophile on TestFlight",
      close: "Close",
      intro: "Request an invitation to the beta. TestFlight is Apple's app for testing apps before release, and it works on iPhone with iOS 17 or later.",
      requiredNote: "Name and email are required.",
      name: "Name",
      email: "Email",
      emailHint: "The invitation is sent here. Use your Apple Account email to install it most easily.",
      servers: "Which servers do you use?",
      serverLabels: {} as Record<string, string>,
      notes: "Anything you would like to tell me?",
      consentBefore: "I agree that my name and email are used only to send the invitation, as described in the",
      consentLink: "privacy policy",
      newTab: "(opens in a new tab)",
      submit: "Send request",
      sending: "Sending…",
      sendingStatus: "Sending your request",
      doneTitle: "Request sent. Thank you!",
      doneText: "You will receive the TestFlight invitation by email. Open it on your iPhone to install the beta.",
      done: "Done",
      offline: "You seem to be offline. Check your connection and try again.",
    },
    errors: {
      name: "Enter your name.",
      nameLength: (max: number) => `Use ${max} characters or fewer.`,
      email: "Enter the email address for your TestFlight invitation.",
      emailFormat: "Enter an email address in the format name@example.com.",
      consent: "Agree to the use of your details to receive the invitation.",
      fields: "Check the highlighted fields.",
      unreadable: "The request could not be read. Try again.",
      rateLimited: "Too many requests. Wait a few minutes and try again.",
      unavailable: "Requests are not available right now. Try again later.",
      failed: "Your request could not be sent. Try again later.",
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
    shots: { group: "Schermate per", switch: "Mostra le schermate per", iphone: "iPhone", desktop: "Desktop" },
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
      names: { mac: "Mac", windows: "Windows", linux: "Linux" },
      version: "Versione",
      notes: "Note di rilascio",
      newTab: "(si apre in una nuova scheda)",
      file: "Scarica",
      copy: "Copia",
      copied: "Copiato",
      options: {
        mac: [["Immagine disco universale", "Apple silicon e Intel, macOS 11 o successivi"]],
        windows: [["Zip portatile", "64 bit, Windows 10 o successivi"]],
        linux: [["Archivio tar", "x86_64, con file .desktop e icona"]],
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
      },
      unavailable: "Questo file non è ancora nell'ultima versione. Tutti i file sono su GitHub.",
      iphone: "Su iPhone?",
      iphoneLink: "Prova la beta",
      web: "Nel browser?",
      webLink: "Ospita il player web",
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
    testflight: {
      button: "Prova su TestFlight",
      title: "Prova Discophile su TestFlight",
      close: "Chiudi",
      intro: "Richiedi un invito alla beta. TestFlight è l'app di Apple per provare le app prima del lancio. Ti serve un iPhone con iOS 17 o versioni successive.",
      requiredNote: "Nome ed email sono obbligatori.",
      name: "Nome",
      email: "Email",
      emailHint: "L'invito arriverà a questo indirizzo. Se usi l'email del tuo Account Apple, l'installazione è più semplice.",
      servers: "Quali server usi?",
      serverLabels: { "Subsonic or OpenSubsonic": "Subsonic o OpenSubsonic", Other: "Altro" } as Record<string, string>,
      notes: "Vuoi aggiungere qualcosa?",
      consentBefore: "Acconsento all'uso di nome ed email al solo scopo di ricevere l'invito, come descritto nell'",
      consentLink: "informativa sulla privacy",
      newTab: "(si apre in una nuova scheda)",
      submit: "Invia richiesta",
      sending: "Invio in corso…",
      sendingStatus: "Invio della richiesta in corso",
      doneTitle: "Richiesta inviata. Grazie!",
      doneText: "Riceverai l'invito a TestFlight via email. Aprilo dal tuo iPhone per installare la beta.",
      done: "Fine",
      offline: "Sembra che tu non sia connesso a internet. Controlla la connessione e riprova.",
    },
    errors: {
      name: "Inserisci il tuo nome.",
      nameLength: (max: number) => `Usa al massimo ${max} caratteri.`,
      email: "Inserisci l'indirizzo email a cui inviare l'invito.",
      emailFormat: "Inserisci un indirizzo email valido, per esempio nome@esempio.it.",
      consent: "Per ricevere l'invito devi acconsentire all'uso dei tuoi dati.",
      fields: "Controlla i campi evidenziati.",
      unreadable: "Impossibile leggere la richiesta. Riprova.",
      rateLimited: "Troppe richieste. Attendi qualche minuto e riprova.",
      unavailable: "Al momento non è possibile inviare richieste. Riprova più tardi.",
      failed: "Impossibile inviare la richiesta. Riprova più tardi.",
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
