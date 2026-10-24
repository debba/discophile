import type { ReactNode } from "react";
import { CodeBlock } from "@/components/CodeBlock";
import { SiteFooter } from "@/components/SiteFooter";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { WEB_GUIDE_URL } from "@/lib/releases";

type Step = { title: string; body: ReactNode; code?: string };

type Copy = {
  title: string;
  lede: string;
  start: Step;
  compose: Step;
  proxy: { title: string; body: ReactNode[]; caption: string; head: [string, string]; rows: [string, string][] };
  https: Step;
  helm: Step & { values: string; after: ReactNode };
  limits: { title: string; items: string[] };
  guide: { title: string; body: string; link: string; devices: string };
};

const RUN = "docker run -d --name discophile -p 8080:8080 \\\n  ghcr.io/debba/discophile-web:latest";

const COMPOSE = `services:
  discophile:
    image: ghcr.io/debba/discophile-web:latest
    restart: unless-stopped
    ports: ["8080:8080"]
    environment:
      DISCOPHILE_PROXY_NAVIDROME: http://navidrome:4533
      DISCOPHILE_PROXY_AUDIOBOOKSHELF: http://audiobookshelf:80

  navidrome:
    image: deluan/navidrome:latest
    volumes: ["./navidrome:/data", "/path/to/music:/music:ro"]

  audiobookshelf:
    image: ghcr.io/advplyr/audiobookshelf:latest
    volumes: ["./abs/config:/config", "./abs/metadata:/metadata", "/path/to/audiobooks:/audiobooks:ro"]`;

const HELM = "helm install discophile oci://ghcr.io/debba/charts/discophile-web \\\n  --namespace media --create-namespace -f values.yaml";

const VALUES = `ingress:
  enabled: true
  className: nginx
  hosts:
    - host: music.example.com
      paths: [{ path: /, pathType: Prefix }]
  tls:
    - secretName: discophile-tls
      hosts: [music.example.com]

proxies:
  navidrome: http://navidrome.media.svc.cluster.local:4533
  audiobookshelf: http://audiobookshelf.media.svc.cluster.local`;

const COPY: Record<Locale, Copy> = {
  en: {
    title: "Your player, on your server.",
    lede: "The web player is Discophile compiled for the browser. Host it next to your music and listen from any computer, with nothing to install on it. It runs as a small, unprivileged container, for amd64 and arm64.",
    start: {
      title: "Start in one line",
      body: <>Run the image, open <code>http://localhost:8080</code> and connect your server. Navidrome works as it is.</>,
      code: RUN,
    },
    compose: {
      title: "Next to your servers, with Docker Compose",
      body: <>Each <code>DISCOPHILE_PROXY_&lt;NAME&gt;</code> serves a server at <code>/proxy/&lt;name&gt;</code> on the player&apos;s own address. In Discophile, add your library as <code>https://music.example.com/proxy/navidrome</code>.</>,
      code: COMPOSE,
    },
    proxy: {
      title: "Why the proxy matters",
      body: [
        "A web page can only talk to another address if that server allows it, which is called CORS. A page served over HTTPS also can't reach a server on plain HTTP, and the equalizer needs audio it is allowed to process.",
        <>The built-in proxy avoids all three. Requests go to the same address as the player, HTTPS covers the whole connection, and streaming, seeking and the equalizer just work. It forwards only to the addresses you set, and its log keeps paths without the query strings where Subsonic carries its token.</>,
      ],
      caption: "Which servers need the proxy",
      head: ["Server", "Without the proxy"],
      rows: [
        ["Navidrome", "Works: it allows requests from any address."],
        ["Subsonic and OpenSubsonic", "Depends on the server. Use the proxy if sign-in fails."],
        ["Jellyfin", "Depends on your Jellyfin and reverse proxy settings. Use the proxy if sign-in fails."],
        ["Audiobookshelf", "Use the proxy: it doesn't allow other addresses by default."],
        ["A local AI model", "Allow the player's address, for example OLLAMA_ORIGINS for Ollama."],
      ],
    },
    https: {
      title: "HTTPS",
      body: "Put the container behind the reverse proxy you already use, such as Caddy, Traefik or nginx, and forward to port 8080. The player needs no special headers and lives at the root of its own address.",
    },
    helm: {
      title: "On Kubernetes, with Helm",
      body: "The chart deploys a stateless, read-only pod with an optional Ingress. Changing the proxies restarts the pods.",
      code: HELM,
      values: VALUES,
      after: <>Then add <code>https://music.example.com/proxy/navidrome</code> in Discophile. Every value of the chart is described in the guide.</>,
    },
    limits: {
      title: "Good to know",
      items: [
        "The web player streams only. Downloads for offline listening and ChatGPT sign-in are on the desktop and iPhone apps.",
        "Credentials are kept in the browser's local storage, so use it on computers you trust.",
        "It doesn't check for updates. Pull a newer image or upgrade the chart; browsers get the new version on the next visit.",
      ],
    },
    guide: {
      title: "The full guide",
      body: "Every option, the Helm values, building the image yourself and hosting the files without Docker.",
      link: "Read the self-hosting guide on GitHub",
      devices: "All downloads",
    },
  },
  it: {
    title: "Il tuo player, sul tuo server.",
    lede: "Il player web è Discophile compilato per il browser. Ospitalo accanto alla tua musica e ascolta da qualsiasi computer, senza installare nulla. Gira come un container piccolo e senza privilegi, per amd64 e arm64.",
    start: {
      title: "Parti con una riga",
      body: <>Avvia l&apos;immagine, apri <code>http://localhost:8080</code> e collega il tuo server. Navidrome funziona così com&apos;è.</>,
      code: RUN,
    },
    compose: {
      title: "Accanto ai tuoi server, con Docker Compose",
      body: <>Ogni <code>DISCOPHILE_PROXY_&lt;NOME&gt;</code> pubblica un server su <code>/proxy/&lt;nome&gt;</code> all&apos;indirizzo del player. In Discophile aggiungi la libreria come <code>https://music.example.com/proxy/navidrome</code>.</>,
      code: COMPOSE,
    },
    proxy: {
      title: "Perché il proxy conta",
      body: [
        "Una pagina web può parlare con un altro indirizzo solo se quel server lo consente, e si chiama CORS. Una pagina servita in HTTPS non può inoltre raggiungere un server in HTTP semplice, e l'equalizzatore ha bisogno di audio che gli sia permesso elaborare.",
        <>Il proxy integrato risolve tutti e tre i problemi. Le richieste vanno allo stesso indirizzo del player, HTTPS copre tutta la connessione, e streaming, avanzamento e equalizzatore funzionano e basta. Inoltra solo agli indirizzi che imposti, e il suo log conserva i percorsi senza le query string in cui Subsonic porta il suo token.</>,
      ],
      caption: "Quali server hanno bisogno del proxy",
      head: ["Server", "Senza il proxy"],
      rows: [
        ["Navidrome", "Funziona: accetta richieste da qualsiasi indirizzo."],
        ["Subsonic e OpenSubsonic", "Dipende dal server. Usa il proxy se l'accesso non riesce."],
        ["Jellyfin", "Dipende dalle impostazioni di Jellyfin e del tuo reverse proxy. Usa il proxy se l'accesso non riesce."],
        ["Audiobookshelf", "Usa il proxy: per impostazione predefinita non accetta altri indirizzi."],
        ["Un modello AI locale", "Consenti l'indirizzo del player, per esempio con OLLAMA_ORIGINS per Ollama."],
      ],
    },
    https: {
      title: "HTTPS",
      body: "Metti il container dietro il reverse proxy che usi già, come Caddy, Traefik o nginx, e inoltra alla porta 8080. Il player non ha bisogno di header particolari e sta alla radice del proprio indirizzo.",
    },
    helm: {
      title: "Su Kubernetes, con Helm",
      body: "Il chart installa un pod senza stato e in sola lettura, con un Ingress facoltativo. Cambiare i proxy riavvia i pod.",
      code: HELM,
      values: VALUES,
      after: <>Poi aggiungi <code>https://music.example.com/proxy/navidrome</code> in Discophile. Tutti i valori del chart sono descritti nella guida.</>,
    },
    limits: {
      title: "Da sapere",
      items: [
        "Il player web funziona solo in streaming. I download per l'ascolto offline e l'accesso a ChatGPT sono nelle app desktop e iPhone.",
        "Le credenziali sono conservate nello storage locale del browser, quindi usalo su computer di cui ti fidi.",
        "Non controlla gli aggiornamenti. Scarica un'immagine più recente o aggiorna il chart; i browser ricevono la nuova versione alla visita successiva.",
      ],
    },
    guide: {
      title: "La guida completa",
      body: "Tutte le opzioni, i valori Helm, come costruire l'immagine da te e come ospitare i file senza Docker.",
      link: "Leggi la guida al self-hosting su GitHub",
      devices: "Tutti i download",
    },
  },
};


export function SelfHostingPage({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  const t = ui[locale].download;
  const code = { code: locale === "en" ? "Code" : "Codice", copy: t.copy, copied: t.copied };
  return (
    <>
      <main id="main" tabIndex={-1}>
        <header className="page-head">
          <h1>{c.title}</h1>
          <p className="lede">{c.lede}</p>
        </header>

        <div className="hosting">
          <section aria-labelledby="start-title">
            <h2 id="start-title">{c.start.title}</h2>
            <p>{c.start.body}</p>
            <CodeBlock code={c.start.code!} title="Docker" labels={code} />
          </section>

          <section aria-labelledby="compose-title">
            <h2 id="compose-title">{c.compose.title}</h2>
            <p>{c.compose.body}</p>
            <CodeBlock code={c.compose.code!} title="compose.yaml" labels={code} />
          </section>

          <section aria-labelledby="proxy-title">
            <h2 id="proxy-title">{c.proxy.title}</h2>
            {c.proxy.body.map((text, i) => <p key={i}>{text}</p>)}
            <div className="table-scroll">
              <table>
                <caption className="sr-only">{c.proxy.caption}</caption>
                <thead><tr>{c.proxy.head.map((cell) => <th key={cell} scope="col">{cell}</th>)}</tr></thead>
                <tbody>
                  {c.proxy.rows.map(([server, text]) => <tr key={server}><th scope="row">{server}</th><td>{text}</td></tr>)}
                </tbody>
              </table>
            </div>
          </section>

          <section aria-labelledby="https-title">
            <h2 id="https-title">{c.https.title}</h2>
            <p>{c.https.body}</p>
          </section>

          <section aria-labelledby="helm-title">
            <h2 id="helm-title">{c.helm.title}</h2>
            <p>{c.helm.body}</p>
            <CodeBlock code={c.helm.values} title="values.yaml" labels={code} />
            <CodeBlock code={c.helm.code!} title="Helm" labels={code} />
            <p>{c.helm.after}</p>
          </section>

          <section aria-labelledby="limits-title">
            <h2 id="limits-title">{c.limits.title}</h2>
            <ul className="facts">{c.limits.items.map((item) => <li key={item}>{item}</li>)}</ul>
          </section>
        </div>

        <section className="next-steps" aria-labelledby="guide-title">
          <h2 id="guide-title">{c.guide.title}</h2>
          <p>{c.guide.body}</p>
          <div className="actions">
            <a className="button primary" href={WEB_GUIDE_URL}>{c.guide.link}</a>
            <a className="button" href={localePath(locale, "/download")}>{c.guide.devices}</a>
          </div>
        </section>
      </main>
      <SiteFooter locale={locale} note="© 2026 Andrea Debernardi" />
    </>
  );
}
