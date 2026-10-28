import type { ReactNode } from "react";
import { CodeBlock } from "@/components/CodeBlock";
import { HostingConfigurator } from "@/components/HostingConfigurator";
import { SiteFooter } from "@/components/SiteFooter";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { WEB_GUIDE_URL } from "@/lib/releases";
import { PROVIDERS, proxyName } from "@/lib/selfHosting";

type Step = { title: string; body: ReactNode; code?: string };

type Copy = {
  title: string;
  lede: string;
  start: Step;
  compose: Step;
  providers: { title: string; body: ReactNode; caption: string; head: [string, string, string]; after: ReactNode; credentials: ReactNode };
  proxy: { title: string; body: ReactNode[]; caption: string; head: [string, string]; rows: [string, string][] };
  https: Step;
  helm: Step & { after: ReactNode };
  limits: { title: string; items: string[] };
  guide: { title: string; body: string; link: string; devices: string };
};

const RUN = "docker run -d --name discophile -p 8080:8080 \\\n  ghcr.io/debba/discophile-web:latest";

const HELM = "helm install discophile oci://ghcr.io/debba/charts/discophile-web \\\n  --namespace media --create-namespace -f values.yaml";

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
      body: <>Use Customize to choose which existing servers to connect and set their addresses and ports. All providers start disabled. Each address must be reachable from the Discophile container: service names work on a shared Docker network; otherwise use your server&apos;s hostname or IP. Include any base path, such as <code>/subsonic</code>.</>,
    },
    providers: {
      title: "From environment variable to library URL",
      body: <>For example, <code>DISCOPHILE_PROXY_JELLYFIN</code> creates <code>/proxy/jellyfin</code>. The suffix is a name you choose, rather than a fixed provider setting: uppercase letters become lowercase and underscores become dashes. Use letters, digits and underscores, with a distinct name for each server. The names below cover all supported library providers.</>,
      caption: "Proxy variables and library URLs for every provider",
      head: ["Provider", "Environment variable", "URL to enter in Discophile"],
      credentials: <>Enter your server&apos;s username and password in Discophile when adding the library. Docker Compose and Helm configure the proxy addresses only; environment variables for library credentials are not currently supported. Credentials do not go in the proxy URL. The web player remembers them in this browser&apos;s local storage.</>,
      after: <>These URLs assume the player is hosted at <code>https://music.example.com</code>; for local use, replace it with <code>http://localhost:8080</code>. Choose the matching library provider in Discophile and sign in with that server&apos;s credentials. You can combine any of these entries. For two Jellyfin servers, for example, use <code>DISCOPHILE_PROXY_JELLYFIN_HOME</code> and <code>DISCOPHILE_PROXY_JELLYFIN_2</code>: their URLs end in <code>/proxy/jellyfin-home</code> and <code>/proxy/jellyfin-2</code>.</>,
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
      body: "The chart deploys a stateless, read-only pod with an optional Ingress. Use Customize to set your domain, Ingress, TLS and proxies. All library providers start disabled; enable the ones you need and replace their service addresses and ports with yours. Proxy names use lowercase letters, digits and dashes, and match the URL paths in the table above. Changing the proxies restarts the pods.",
      code: HELM,
      after: <>After enabling a proxy, add its library URL in Discophile, for example <code>https://music.example.com/proxy/navidrome</code>. Every value of the chart is described in the guide.</>,
    },
    limits: {
      title: "Good to know",
      items: [
        "The web player streams only. Downloads for offline listening and ChatGPT sign-in are on the desktop, iPhone and Android apps.",
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
      body: <>Usa Personalizza per scegliere quali server esistenti collegare e impostare i loro indirizzi e le porte. Tutti i provider sono inizialmente disabilitati. Ogni indirizzo deve essere raggiungibile dal container Discophile: i nomi dei servizi funzionano su una rete Docker condivisa; altrimenti usa il nome host o l&apos;IP del server. Includi l&apos;eventuale percorso di base, come <code>/subsonic</code>.</>,
    },
    providers: {
      title: "Dalla variabile d’ambiente all’URL della libreria",
      body: <>Per esempio, <code>DISCOPHILE_PROXY_JELLYFIN</code> crea <code>/proxy/jellyfin</code>. Il suffisso è un nome che scegli tu, non un&apos;impostazione fissa del provider: le maiuscole diventano minuscole e gli underscore diventano trattini. Usa lettere, cifre e underscore, con un nome distinto per ogni server. I nomi qui sotto coprono tutti i provider di librerie supportati.</>,
      caption: "Variabili proxy e URL delle librerie per ogni provider",
      head: ["Provider", "Variabile d’ambiente", "URL da inserire in Discophile"],
      credentials: <>Inserisci il nome utente e la password del server in Discophile quando aggiungi la libreria. Docker Compose e Helm configurano solo gli indirizzi dei proxy; al momento non sono supportate variabili d&apos;ambiente per le credenziali delle librerie. Le credenziali non vanno nell&apos;URL del proxy. Il player web le ricorda nello storage locale di questo browser.</>,
      after: <>Questi URL presuppongono che il player sia su <code>https://music.example.com</code>; in locale sostituiscilo con <code>http://localhost:8080</code>. Scegli il provider corrispondente in Discophile e accedi con le credenziali di quel server. Puoi combinare liberamente queste voci. Per due server Jellyfin, per esempio, usa <code>DISCOPHILE_PROXY_JELLYFIN_HOME</code> e <code>DISCOPHILE_PROXY_JELLYFIN_2</code>: i rispettivi URL terminano con <code>/proxy/jellyfin-home</code> e <code>/proxy/jellyfin-2</code>.</>,
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
      body: "Il chart installa un pod senza stato e in sola lettura, con un Ingress facoltativo. Usa Personalizza per impostare dominio, Ingress, TLS e proxy. Tutti i provider di librerie sono inizialmente disabilitati: abilita quelli che ti servono e sostituisci gli indirizzi e le porte dei servizi con i tuoi. I nomi dei proxy usano lettere minuscole, cifre e trattini e corrispondono ai percorsi URL della tabella sopra. Cambiare i proxy riavvia i pod.",
      code: HELM,
      after: <>Dopo aver abilitato un proxy, aggiungi il suo URL di libreria in Discophile, per esempio <code>https://music.example.com/proxy/navidrome</code>. Tutti i valori del chart sono descritti nella guida.</>,
    },
    limits: {
      title: "Da sapere",
      items: [
        "Il player web funziona solo in streaming. I download per l'ascolto offline e l'accesso a ChatGPT sono nelle app desktop, iPhone e Android.",
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
            <HostingConfigurator mode="compose" locale={locale} />
          </section>

          <section aria-labelledby="providers-title">
            <h2 id="providers-title">{c.providers.title}</h2>
            <p>{c.providers.body}</p>
            <div className="table-scroll" tabIndex={0} role="region" aria-label={c.providers.caption}>
              <table>
                <caption className="sr-only">{c.providers.caption}</caption>
                <thead><tr>{c.providers.head.map((cell) => <th key={cell} scope="col">{cell}</th>)}</tr></thead>
                <tbody>
                  {PROVIDERS.map(({ name, key }) => (
                    <tr key={key}>
                      <th scope="row">{name}</th>
                      <td><code>DISCOPHILE_PROXY_{key}</code></td>
                      <td><code>https://music.example.com/proxy/{proxyName(key)}</code></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p>{c.providers.after}</p>
            <p>{c.providers.credentials}</p>
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
            <HostingConfigurator mode="helm" locale={locale} />
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
