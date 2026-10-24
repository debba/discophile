import type { Metadata } from "next";
import { CookiePreferencesButton } from "@/components/CookieConsent";
import { PolicyTable } from "@/components/PolicyTable";
import { SiteFooter } from "@/components/SiteFooter";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Come il sito di Discophile usa i cookie e le tecnologie simili, e come puoi controllarli.";

export const metadata: Metadata = pageMetadata({
  locale: "it",
  path: "/cookie-policy",
  title: "Cookie policy | Discophile",
  socialTitle: "Cookie policy di Discophile",
  description,
  type: "article",
  imageAlt: OG_ALT.it,
});

export default function CookiePolicy() {
  return (
    <>
      <main id="main" className="policy" tabIndex={-1}>
        <h1>Cookie policy</h1>
        <p className="updated">Ultimo aggiornamento: 24 ottobre 2026</p>

        <p className="policy-summary">Il sito di Discophile conta le visite con Matomo, ospitato dallo sviluppatore, senza cookie. I cookie di misurazione vengono impostati solo se li consenti. Non ci sono pubblicità né marketing, e nulla viene condiviso con terze parti.</p>

        <h2>Cosa sono i cookie?</h2>
        <p>I cookie sono piccoli file di testo che un sito salva nel browser, per ricordare informazioni tra una pagina e l&apos;altra e tra una visita e l&apos;altra. Le tecnologie simili, come il local storage, funzionano allo stesso modo. Questa policy le copre tutte.</p>

        <h2>Come questo sito usa i cookie</h2>
        <p>Il sito usa un insieme minimo di cookie e tecnologie simili, raggruppati nelle categorie qui sotto. Pagine, immagini, caratteri e video di presentazione sono serviti dal sito stesso. Il sito ricorda la lingua e l&apos;aspetto che scegli. Puoi cambiare la tua scelta in qualsiasi momento con <b>Preferenze cookie</b> in fondo a ogni pagina.</p>

        <h2>Categorie di cookie</h2>
        <h3 className="policy-category">Necessari <span className="badge">Sempre attivi</span></h3>
        <p>Sono indispensabili al funzionamento del sito e non si possono disattivare. Questa categoria comprende le <b>statistiche sulle pagine visitate senza cookie</b> con <a href="https://matomo.org">Matomo</a>, ospitato dallo sviluppatore: non viene impostato alcun cookie, l&apos;indirizzo IP viene anonimizzato prima di essere salvato e nessun dato è condiviso con terze parti. Il trattamento si basa sul legittimo interesse a conoscere il traffico in forma aggregata e anonima.</p>
        <PolicyTable
          locale="it"
          id="necessary"
          caption="Cookie e tecnologie simili necessari"
          rows={[
            ["discophile-cookie-consent", "www.discophile.app", "Salva la tua scelta sui cookie", "Persistente (local storage)"],
            ["discophile-locale", "www.discophile.app", "Ricorda la lingua che hai scelto con il selettore. Senza questo cookie, il sito segue la lingua del browser", "12 mesi"],
            ["discophile-theme", "www.discophile.app", "Ricorda l'aspetto che hai scelto, Chiaro o Scuro. Non viene salvato con Automatico, che segue il tuo dispositivo", "Persistente (local storage)"],
            ["Nessuno (senza cookie)", "analytics.debbaweb.it", "Statistiche anonime sulle pagine visitate senza identificativi persistenti, più il conteggio delle aperture della finestra di download, della piattaforma scaricata, del video di presentazione, delle richieste TestFlight, delle iscrizioni alla newsletter e dei link a GitHub Sponsors", "Solo sessione"],
            ["Nessuno (senza cookie)", "www.discophile.app", "Protezione antispam dei moduli TestFlight e newsletter: le richieste dal tuo indirizzo IP sono contate nella memoria del server, mai scritte su disco", "10 minuti"],
            ["Nessuno (senza cookie)", "Vercel", "Hosting: serve le pagine e protegge il servizio, trattando il tuo indirizzo IP nei log del server", "Secondo la conservazione dei log di Vercel"],
          ]}
        />

        <h3 className="policy-category">Misurazione <span className="badge">Facoltativi</span></h3>
        <p>Se consenti i cookie di misurazione, Matomo può riconoscere chi torna a visitare il sito e misurare l&apos;interesse nel tempo. Gli indirizzi IP restano anonimizzati e nessun dato è condiviso con terze parti. Senza il tuo permesso questi cookie non vengono mai impostati, e revocarlo li cancella.</p>
        <PolicyTable
          locale="it"
          id="measurement"
          caption="Cookie di misurazione"
          rows={[
            ["_pk_id.*", "analytics.debbaweb.it", "Riconosce un visitatore che torna, tra una visita e l'altra", "13 mesi"],
            ["_pk_ses.*", "analytics.debbaweb.it", "Raggruppa le pagine di una singola visita", "30 minuti"],
            ["_pk_ref.*", "analytics.debbaweb.it", "Salva il sito da cui arriva la visita", "6 mesi"],
          ]}
        />

        <h3 className="policy-category">Marketing</h3>
        <p>Non ci sono cookie di marketing. Se un giorno venisse aggiunto un servizio che li richiede, questa policy sarà aggiornata prima, e il sito chiederà il tuo consenso prima di impostarli.</p>

        <h2>Moduli</h2>
        <p>Il modulo di richiesta TestFlight e l&apos;iscrizione alla newsletter non impostano cookie. Una richiesta TestFlight viene consegnata allo sviluppatore via email. Un&apos;iscrizione alla newsletter viene inoltrata dal server di questo sito a Emailchef, che poi ti invia un&apos;email di conferma; su questo sito non viene caricato alcuno script o cookie di Emailchef. Entrambi sono descritti nell&apos;<a href="/it/privacy#website">informativa sulla privacy</a>.</p>

        <h2>Download</h2>
        <p>L&apos;elenco delle versioni viene letto da GitHub dal server di questo sito, quindi aprire la finestra di download non contatta GitHub dal tuo browser. I file vengono scaricati da GitHub quando ne scegli uno.</p>

        <h2>Link ad altri siti</h2>
        <p>I link a GitHub (download, codice sorgente e GitHub Sponsors), all&apos;Arch User Repository, agli altri progetti dello sviluppatore e ai servizi Apple portano a siti con le proprie cookie policy. Nulla viene caricato da quei siti finché non segui un link o avvii un download.</p>

        <h2>Gestire le preferenze</h2>
        <p>La tua scelta è salvata solo nel tuo browser e non viene mai inviata a un server. Puoi cambiarla in qualsiasi momento, oppure cancellare i dati di questo sito nel browser perché ti venga chiesto di nuovo.</p>
        <p><CookiePreferencesButton locale="it" /></p>

        <h2>Contatti</h2>
        <p>Per domande su questa cookie policy, scrivi a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>
      </main>

      <SiteFooter locale="it" current="cookie-policy" note="© 2026 Andrea Debernardi" />
    </>
  );
}
