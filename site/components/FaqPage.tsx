import { SiteFooter } from "@/components/SiteFooter";
import { TestFlightButton } from "@/components/TestFlight";
import { localePath, type Locale } from "@/lib/i18n";
import { REPO_URL } from "@/lib/releases";
import { SITE_URL } from "@/lib/seo";

/** A link under an answer: a page or address, or the TestFlight invitation dialog. */
type FaqLink = { label: string; href: string } | { label: string; testflight: true };
/** Answers are plain text, so the same words feed the page and the FAQPage data for search engines. */
type Question = { id: string; q: string; a: string[]; links?: FaqLink[] };
type Group = { id: string; title: string; questions: Question[] };

type Copy = {
  title: string;
  lede: string;
  groups: Group[];
  more: { title: string; body: string; email: string; issues: string };
};

const EMAIL = "andrea@debbaweb.it";
const ISSUES_URL = `${REPO_URL}/issues`;

const COPY: Record<Locale, Copy> = {
  en: {
    title: "Questions, answered.",
    lede: "What Discophile needs, what it does with your data, and how to get it on each device.",
    groups: [
      {
        id: "basics",
        title: "The basics",
        questions: [
          {
            id: "what",
            q: "What is Discophile?",
            a: ["A music player for the music you host yourself. It plays your Navidrome, Subsonic and Jellyfin libraries, Audiobookshelf audiobooks and internet radio, on iPhone, Mac, Windows, Linux and in the browser."],
            links: [{ label: "See everything Discophile does", href: "/features" }],
          },
          {
            id: "servers",
            q: "Which servers does it work with?",
            a: ["Navidrome, Subsonic and OpenSubsonic, and Jellyfin for music, Audiobookshelf for audiobooks, and any internet radio station you add. You can connect more than one library and switch between them."],
          },
          {
            id: "own-server",
            q: "Do I need my own server?",
            a: ["Yes. Discophile doesn't sell or stream music: it plays the library on a server you run or have access to. Internet radio is the exception, and works without one."],
          },
          {
            id: "account",
            q: "Do I need a Discophile account?",
            a: ["No. There is no Discophile account and no Discophile server. The app connects directly to the servers you add, with the credentials you already use for them."],
          },
          {
            id: "free",
            q: "Is it free?",
            a: ["The apps for Mac, Windows and Linux and the web player are free and open source. The iPhone app is coming soon to the App Store, and you can try the beta now."],
            links: [{ label: "Source code on GitHub", href: REPO_URL }],
          },
        ],
      },
      {
        id: "devices",
        title: "Devices and downloads",
        questions: [
          {
            id: "requirements",
            q: "What do I need to run it?",
            a: ["iPhone: iOS 17 or later. Mac: macOS 11 or later, on Apple silicon or Intel. Windows: Windows 10 or later, 64-bit. Linux: x86_64, with PipeWire, PulseAudio or ALSA. The web player runs on a server you host with Docker or Kubernetes, and opens in any modern browser."],
            links: [{ label: "All downloads and requirements", href: "/download" }],
          },
          {
            id: "iphone-beta",
            q: "How can I try the iPhone app now?",
            a: ["Request an invitation to the beta. It runs through TestFlight, Apple's app for trying apps before they launch, and needs an iPhone with iOS 17 or later."],
            links: [{ label: "Request a TestFlight invitation", testflight: true }],
          },
          {
            id: "android",
            q: "Is there an Android app?",
            a: ["Not at the moment. If you host the web player, you can listen from the browser."],
            links: [{ label: "Host the web player", href: "/self-hosting" }],
          },
          {
            id: "offline",
            q: "Can I listen offline?",
            a: ["Yes, on iPhone and on the desktop: download albums, playlists and whole audiobooks and they play without a connection. The web player streams only."],
          },
          {
            id: "updates",
            q: "How do I update the desktop app?",
            a: ["Discophile tells you when a new version is out and opens the right download. On Arch Linux you can install and update discophile-bin from the AUR. The Mac app is signed and notarized by Apple."],
          },
        ],
      },
      {
        id: "privacy",
        title: "Privacy and the assistant",
        questions: [
          {
            id: "data",
            q: "What happens to my data?",
            a: ["Your passwords stay in your device's keychain, and your listening history stays on your device. The apps have no advertising, no analytics and no tracking, and the developer never receives your library or credentials."],
            links: [{ label: "Read the privacy policy", href: "/privacy" }],
          },
          {
            id: "assistant",
            q: "How does the music assistant work?",
            a: ["Describe what you want to hear and it picks songs from your library, so everything it suggests can be played right away. On iPhone it uses Apple Intelligence on the device, on the desktop and in the web player a local model such as Ollama or LM Studio, and on iPhone and desktop you can also connect your ChatGPT account.", "Changes to your server, such as editing a playlist, are applied only after you confirm them."],
          },
          {
            id: "chatgpt",
            q: "Do I need ChatGPT?",
            a: ["No, it's optional. If you connect it, OpenAI receives your request and song metadata such as title, artist and album, never your audio, passwords, server addresses or listening history."],
          },
        ],
      },
      {
        id: "web",
        title: "The web player",
        questions: [
          {
            id: "web-what",
            q: "What is the web player?",
            a: ["The same Discophile, compiled for the browser. You host it next to your music server, as a small container for amd64 and arm64, and listen from any computer without installing anything. The developer doesn't run a public instance."],
            links: [{ label: "Host the web player", href: "/self-hosting" }],
          },
          {
            id: "proxy",
            q: "Why does it include a proxy?",
            a: ["A web page can only talk to another address if that server allows it, and a page on HTTPS can't reach a server on plain HTTP. The built-in proxy sends everything through the player's own address, so sign-in, streaming, seeking and the equalizer work. Navidrome works even without it; Audiobookshelf needs it."],
          },
        ],
      },
    ],
    more: {
      title: "Still have a question?",
      body: "Write to me, or open an issue on GitHub for bugs and ideas.",
      email: `Write to ${EMAIL}`,
      issues: "Open an issue on GitHub",
    },
  },
  it: {
    title: "Domande e risposte.",
    lede: "Cosa serve a Discophile, cosa fa con i tuoi dati e come averlo su ogni dispositivo.",
    groups: [
      {
        id: "basics",
        title: "Le basi",
        questions: [
          {
            id: "what",
            q: "Cos'è Discophile?",
            a: ["Un player per la musica che ospiti tu. Riproduce le tue librerie Navidrome, Subsonic e Jellyfin, gli audiolibri di Audiobookshelf e le radio online, su iPhone, Mac, Windows, Linux e nel browser."],
            links: [{ label: "Scopri tutto quello che fa Discophile", href: "/features" }],
          },
          {
            id: "servers",
            q: "Con quali server funziona?",
            a: ["Navidrome, Subsonic e OpenSubsonic e Jellyfin per la musica, Audiobookshelf per gli audiolibri, e qualsiasi radio online che aggiungi. Puoi collegare più librerie e passare dall'una all'altra."],
          },
          {
            id: "own-server",
            q: "Mi serve un server mio?",
            a: ["Sì. Discophile non vende né trasmette musica: riproduce la libreria di un server che gestisci tu o a cui hai accesso. Fanno eccezione le radio online, che funzionano anche senza."],
          },
          {
            id: "account",
            q: "Devo creare un account Discophile?",
            a: ["No. Non esistono un account né un server Discophile. L'app si collega direttamente ai server che aggiungi, con le credenziali che usi già."],
          },
          {
            id: "free",
            q: "È gratis?",
            a: ["Le app per Mac, Windows e Linux e il player web sono gratuiti e open source. L'app per iPhone arriva presto sull'App Store, e puoi già provare la beta."],
            links: [{ label: "Codice sorgente su GitHub", href: REPO_URL }],
          },
        ],
      },
      {
        id: "devices",
        title: "Dispositivi e download",
        questions: [
          {
            id: "requirements",
            q: "Cosa mi serve per usarlo?",
            a: ["iPhone: iOS 17 o versioni successive. Mac: macOS 11 o versioni successive, con Apple silicon o Intel. Windows: Windows 10 o versioni successive, a 64 bit. Linux: x86_64, con PipeWire, PulseAudio o ALSA. Il player web gira su un server che ospiti tu con Docker o Kubernetes, e si apre in qualsiasi browser moderno."],
            links: [{ label: "Tutti i download e i requisiti", href: "/download" }],
          },
          {
            id: "iphone-beta",
            q: "Come posso provare subito l'app per iPhone?",
            a: ["Richiedi un invito alla beta. Passa da TestFlight, l'app di Apple per provare le app prima del lancio, e ti serve un iPhone con iOS 17 o versioni successive."],
            links: [{ label: "Richiedi un invito a TestFlight", testflight: true }],
          },
          {
            id: "android",
            q: "C'è un'app per Android?",
            a: ["Al momento no. Se ospiti il player web, puoi ascoltare dal browser."],
            links: [{ label: "Ospita il player web", href: "/self-hosting" }],
          },
          {
            id: "offline",
            q: "Posso ascoltare offline?",
            a: ["Sì, su iPhone e sul computer: scarica album, playlist e audiolibri interi e li ascolti anche senza connessione. Il player web funziona solo in streaming."],
          },
          {
            id: "updates",
            q: "Come aggiorno l'app per computer?",
            a: ["Discophile ti avvisa quando esce una nuova versione e apre il download giusto. Su Arch Linux puoi installare e aggiornare discophile-bin dall'AUR. L'app per Mac è firmata e autenticata da Apple."],
          },
        ],
      },
      {
        id: "privacy",
        title: "Privacy e assistente",
        questions: [
          {
            id: "data",
            q: "Che fine fanno i miei dati?",
            a: ["Le password restano nel portachiavi del dispositivo e la cronologia di ascolto resta sul dispositivo. Le app non hanno pubblicità, statistiche di utilizzo né tracciamento, e lo sviluppatore non riceve mai la tua libreria o le tue credenziali."],
            links: [{ label: "Leggi l'informativa sulla privacy", href: "/privacy" }],
          },
          {
            id: "assistant",
            q: "Come funziona l'assistente musicale?",
            a: ["Descrivi cosa vuoi ascoltare e sceglie i brani dalla tua libreria, così tutto quello che propone si può riprodurre subito. Su iPhone usa Apple Intelligence sul dispositivo, sul computer e nel player web un modello locale come Ollama o LM Studio, e su iPhone e computer puoi anche collegare il tuo account ChatGPT.", "Le modifiche al tuo server, come cambiare una playlist, vengono applicate solo dopo la tua conferma."],
          },
          {
            id: "chatgpt",
            q: "Mi serve ChatGPT?",
            a: ["No, è facoltativo. Se lo colleghi, OpenAI riceve la tua richiesta e i metadati dei brani come titolo, artista e album, mai audio, password, indirizzi dei server o cronologia di ascolto."],
          },
        ],
      },
      {
        id: "web",
        title: "Il player web",
        questions: [
          {
            id: "web-what",
            q: "Cos'è il player web?",
            a: ["Lo stesso Discophile, compilato per il browser. Lo ospiti accanto al tuo server musicale, come un piccolo container per amd64 e arm64, e ascolti da qualsiasi computer senza installare nulla. Lo sviluppatore non gestisce un'istanza pubblica."],
            links: [{ label: "Ospita il player web", href: "/self-hosting" }],
          },
          {
            id: "proxy",
            q: "Perché include un proxy?",
            a: ["Una pagina web può parlare con un altro indirizzo solo se quel server lo consente, e una pagina in HTTPS non può raggiungere un server in HTTP semplice. Il proxy integrato fa passare tutto dall'indirizzo del player, così accesso, streaming, salti e equalizzatore funzionano. Navidrome funziona anche senza; Audiobookshelf ne ha bisogno."],
          },
        ],
      },
    ],
    more: {
      title: "Hai ancora una domanda?",
      body: "Scrivimi, oppure apri una segnalazione su GitHub per bug e idee.",
      email: `Scrivi a ${EMAIL}`,
      issues: "Apri una segnalazione su GitHub",
    },
  },
};

function href(locale: Locale, target: string) {
  return target.startsWith("/") ? localePath(locale, target) : target;
}

export function FaqPage({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  const data = {
    "@context": "https://schema.org",
    "@type": "FAQPage",
    url: `${SITE_URL}${localePath(locale, "/faq")}`,
    inLanguage: locale,
    mainEntity: c.groups.flatMap((group) =>
      group.questions.map((item) => ({
        "@type": "Question",
        name: item.q,
        acceptedAnswer: { "@type": "Answer", text: item.a.join(" ") },
      })),
    ),
  };

  return (
    <>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(data).replace(/</g, "\\u003c") }} />
      <main id="main" tabIndex={-1}>
        <header className="page-head">
          <h1>{c.title}</h1>
          <p className="lede">{c.lede}</p>
        </header>

        <div className="faq">
          {c.groups.map((group) => (
            <section key={group.id} aria-labelledby={`${group.id}-title`}>
              <h2 id={`${group.id}-title`}>{group.title}</h2>
              {group.questions.map((item) => (
                <details key={item.id} id={item.id} className="faq-item">
                  <summary><h3>{item.q}</h3></summary>
                  <div className="faq-answer">
                    {item.a.map((text) => <p key={text}>{text}</p>)}
                    {item.links && (
                      <p className="faq-links">
                        {item.links.map((link) =>
                          "testflight" in link
                            ? <TestFlightButton key={link.label} className="link-button">{link.label}</TestFlightButton>
                            : <a key={link.label} href={href(locale, link.href)}>{link.label}</a>,
                        )}
                      </p>
                    )}
                  </div>
                </details>
              ))}
            </section>
          ))}
        </div>

        <section className="next-steps" aria-labelledby="more-title">
          <h2 id="more-title">{c.more.title}</h2>
          <p>{c.more.body}</p>
          <div className="actions">
            <a className="button primary" href={`mailto:${EMAIL}`}>{c.more.email}</a>
            <a className="button" href={ISSUES_URL}>{c.more.issues}</a>
          </div>
        </section>
      </main>
      <SiteFooter locale={locale} note="© 2026 Andrea Debernardi" />
    </>
  );
}
