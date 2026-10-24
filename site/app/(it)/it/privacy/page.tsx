import type { Metadata } from "next";
import { SiteFooter } from "@/components/SiteFooter";
import { alternates } from "@/lib/i18n";

const description = "Come Discophile gestisce i tuoi server, la cronologia di ascolto, le funzioni AI facoltative e i servizi online.";

export const metadata: Metadata = {
  title: "Informativa sulla privacy | Discophile",
  description,
  alternates: { ...alternates("/privacy"), canonical: "/it/privacy" },
  openGraph: { type: "article", url: "/it/privacy", title: "Informativa sulla privacy di Discophile", description, locale: "it_IT" },
};

export default function Privacy() {
  return (
    <>
      <main id="main" className="policy" tabIndex={-1}>
        <h1>Informativa sulla privacy</h1>
        <p className="updated">Ultimo aggiornamento: 22 ottobre 2026</p>

        <p className="policy-summary">Discophile non ha account né un server proprio. Collega il tuo iPhone direttamente ai server di musica e audiolibri che scegli tu. La cronologia di ascolto resta sul tuo iPhone, e nell&apos;app non ci sono pubblicità, strumenti di analisi o tracciamento.</p>

        <h2>Titolare del trattamento</h2>
        <p>Discophile è sviluppato da Andrea Debernardi. Per richieste sulla privacy e assistenza, scrivi a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>

        <h2>I tuoi server e la tua libreria</h2>
        <p>Discophile è un client per server Subsonic e OpenSubsonic, Navidrome, Jellyfin e Audiobookshelf, e per le stazioni radio online che aggiungi. Invia richieste direttamente agli indirizzi che inserisci, per sfogliare la libreria, riprodurre e scaricare l&apos;audio, caricare copertine e testi, gestire playlist e preferiti e sincronizzare la posizione degli audiolibri. È il gestore di ciascun server a decidere cosa registra, per esempio nei log di accesso.</p>
        <p>Password, chiavi API e token di accesso sono salvati nel portachiavi di iOS e vengono inviati solo al server a cui appartengono. Le connessioni usano HTTPS per impostazione predefinita; HTTP semplice viene usato solo se lo consenti esplicitamente per un server locale di fiducia.</p>

        <h2>Dati conservati sul tuo iPhone</h2>
        <p>Per costruire Ascolta, i mix di scoperta, gli album suggeriti e le statistiche di ascolto, Discophile registra ciò che ascolti su questo iPhone, separatamente per ogni libreria. Gli ascolti giornalieri sono conservati per 400 giorni. Sul dispositivo sono salvati anche code, selezioni, chat con l&apos;assistente, musica e audiolibri scaricati e testi in cache.</p>
        <p>Cronologia di ascolto, statistiche, chat con l&apos;assistente e download sono salvati in file protetti esclusi dai backup del dispositivo. Le normali preferenze possono essere incluse nei backup, secondo le impostazioni del tuo dispositivo Apple. Puoi consultare o cancellare la cronologia di ascolto in Impostazioni.</p>

        <h2>Assistente musicale e AI facoltativa</h2>
        <p><b>Su questo iPhone</b> usa Apple Intelligence sui dispositivi supportati. Le richieste sono elaborate sul dispositivo, e Discophile non passa mai di sua iniziativa a un servizio cloud.</p>
        <p><b>ChatGPT</b> è facoltativo e funziona solo dopo che hai collegato il tuo account ChatGPT. Quando invii una richiesta con ChatGPT selezionato, Discophile invia a OpenAI il testo della richiesta e una parte limitata della conversazione recente, compresi i messaggi scritti prima nella stessa chat. Può inviare anche titolo, artista e album del brano in riproduzione, i nomi delle tue raccolte salvate e un breve riepilogo della selezione che stai modificando. Per scegliere i brani invia titolo, artista, album, genere, anno e durata di un massimo di 200 brani candidati, identificati da ID temporanei.</p>
        <p>Discophile non invia a OpenAI il tuo audio, gli indirizzi dei server, le credenziali dei server, gli identificativi privati della libreria o le registrazioni del microfono. Le richieste sono fatte con l&apos;archiviazione disattivata, ma OpenAI le elabora secondo i propri termini, la propria informativa sulla privacy e le impostazioni del tuo account, e questo non garantisce che nessun dato venga conservato. Puoi scollegare ChatGPT in Impostazioni in qualsiasi momento. L&apos;azione facoltativa che copia una richiesta e apre ChatGPT condivide solo ciò che scegli di incollare.</p>
        <p>Le modifiche che l&apos;assistente propone al tuo server, come modificare una playlist, vengono applicate solo dopo la tua conferma.</p>

        <h2>Dettatura, Siri e notifiche</h2>
        <p>La dettatura usa il riconoscimento vocale sul dispositivo e richiede i permessi per microfono e riconoscimento vocale. L&apos;audio del microfono non viene salvato né inviato a OpenAI o allo sviluppatore. Le richieste a Siri sono gestite da Apple secondo le tue impostazioni di Siri, poi risolte nella libreria selezionata. Le notifiche sulle risposte dell&apos;assistente sono generate localmente sull&apos;iPhone; Discophile non usa un servizio di notifiche push remoto.</p>

        <h2>Servizi online che puoi scegliere di usare</h2>
        <p>Alcune funzioni contattano servizi pubblici solo quando le usi. Ciascuno riceve le informazioni di rete necessarie a rispondere, come il tuo indirizzo IP, ed è regolato dalla propria informativa sulla privacy.</p>
        <ul>
          <li><b>Testi online</b>: è disattivata per impostazione predefinita. Se la attivi e apri il testo di un brano che il tuo server non ha, Discophile invia a LRCLIB titolo, artista, album e durata.</li>
          <li><b>Ricerca e link dei podcast</b>: Discophile invia alla directory di Apple Podcasts solo il testo che cerchi o il link che incolli. Per il link di uno show Spotify, legge la pagina pubblica dello show per ricavarne il titolo.</li>
          <li><b>Correzione per le cuffie</b>: Discophile scarica da GitHub l&apos;elenco dei profili AutoEq e il profilo che scegli. Non invia alcun dato sulla libreria o sugli ascolti.</li>
        </ul>
        <p>Le credenziali dei server non vengono mai inviate a questi servizi.</p>

        <h2>Nessun tracciamento</h2>
        <p>Discophile non include SDK pubblicitari o di analisi, non vende dati personali e non ti traccia tra app e siti web. La modalità sport controlla solo la riproduzione musicale; non registra allenamenti e non legge dati sanitari.</p>

        <h2>Conservazione e scelte</h2>
        <p>Rimuovere una libreria in Impostazioni elimina dall&apos;iPhone la connessione e i relativi download. Non cancella nulla dal tuo server. Scollegare ChatGPT rimuove le sue credenziali dal dispositivo; le richieste già inviate a OpenAI vanno gestite nel tuo account OpenAI.</p>
        <p>Eliminare l&apos;app rimuove i suoi dati locali. Gli elementi del portachiavi possono sopravvivere all&apos;app, quindi rimuovi le librerie e scollega ChatGPT prima di eliminarla se vuoi cancellare le credenziali salvate. Con «Rimuovi app», invece, i dati restano sul dispositivo. I backup del dispositivo si gestiscono nelle impostazioni Apple.</p>

        <h2 id="website">Questo sito web</h2>
        <p>Il sito di Discophile non contiene pubblicità e non ti traccia su altri siti. Conta le visite con Matomo, ospitato dallo sviluppatore, senza cookie e con indirizzi IP anonimizzati; i cookie di misurazione vengono impostati solo se li consenti. La <a href="/it/cookie-policy">cookie policy</a> elenca tutto ciò che il sito salva. Caratteri e contenuti multimediali sono serviti dal sito stesso. Il sito è ospitato su Vercel, che tratta informazioni tecniche come il tuo indirizzo IP per servire le pagine e proteggere il servizio. Quando richiedi un invito a TestFlight, il modulo invia allo sviluppatore via email nome, indirizzo email, i server selezionati e il messaggio facoltativo. Questi dati sono usati solo per invitarti alla beta e risponderti. Gli inviti a TestFlight sono consegnati da Apple, che gestisce poi la tua partecipazione secondo i propri termini e la propria informativa sulla privacy.</p>
        <p>Puoi chiedere in qualsiasi momento la cancellazione di questi dati scrivendo a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. Uscire dalla beta nell&apos;app TestFlight interrompe l&apos;invio di nuove build.</p>

        <h2>Richieste sulla privacy</h2>
        <p>Lo sviluppatore non riceve la tua libreria, la cronologia di ascolto o le credenziali, quindi la maggior parte dei dati si gestisce solo sul tuo iPhone o sui tuoi server. Se contatti l&apos;assistenza o richiedi un invito a TestFlight, il tuo indirizzo email e ciò che scrivi sono usati solo per rispondere e risolvere la richiesta. Non inserire password o token nelle richieste di assistenza.</p>
        <p>A seconda di dove vivi, puoi avere il diritto di accedere ai dati personali, rettificarli o cancellarli, opporti al trattamento o limitarlo, chiedere la portabilità, revocare il consenso e presentare reclamo all&apos;autorità di protezione dei dati, in Italia il Garante per la protezione dei dati personali. Per esercitare questi diritti sui dati in possesso dello sviluppatore, scrivi a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. Per i dati in possesso del gestore del tuo server o di OpenAI, contattali direttamente.</p>

        <h2>Modifiche</h2>
        <p>Gli aggiornamenti a questa informativa saranno pubblicati su questa pagina con una nuova data, e riportati nelle versioni successive dell&apos;app.</p>
      </main>

      <SiteFooter locale="it" current="privacy" note="© 2026 Andrea Debernardi" />
    </>
  );
}
