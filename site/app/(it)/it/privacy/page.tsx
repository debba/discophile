import type { Metadata } from "next";
import { SiteFooter } from "@/components/SiteFooter";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Come Discophile per iPhone, desktop e web gestisce i tuoi server, la cronologia di ascolto, le funzioni AI facoltative e i servizi online.";

export const metadata: Metadata = pageMetadata({
  locale: "it",
  path: "/privacy",
  title: "Informativa sulla privacy | Discophile",
  socialTitle: "Informativa sulla privacy di Discophile",
  description,
  type: "article",
  imageAlt: OG_ALT.it,
});

export default function Privacy() {
  return (
    <>
      <main id="main" className="policy" tabIndex={-1}>
        <h1>Informativa sulla privacy</h1>
        <p className="updated">Ultimo aggiornamento: 24 ottobre 2026</p>

        <p className="policy-summary">Discophile si collega direttamente ai server musicali e di audiolibri che scegli. Per ascoltare non serve un account. Discophile Connect è un account e un servizio desktop cifrato facoltativo. Le app non contengono pubblicità, analisi o tracciamento.</p>

        <p>Questa informativa riguarda Discophile per iPhone, Discophile per Mac, Windows e Linux (l&apos;app desktop), il player web che puoi ospitare tu e questo sito. Salvo dove indicato diversamente, vale per tutti.</p>

        <h2>Titolare del trattamento</h2>
        <p>Discophile è sviluppato da Andrea Debernardi. Per richieste sulla privacy e assistenza, scrivi a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>

        <h2>I tuoi server e la tua libreria</h2>
        <p>Discophile è un client per server Subsonic e OpenSubsonic, Navidrome, Jellyfin e Audiobookshelf, e per le stazioni radio online che aggiungi. Invia richieste direttamente agli indirizzi che inserisci, per sfogliare la libreria, riprodurre e scaricare l&apos;audio, caricare copertine e testi, gestire playlist e preferiti e sincronizzare la posizione degli audiolibri. È il gestore di ciascun server a decidere cosa registra, per esempio nei log di accesso.</p>
        <p>Password, chiavi API e token di accesso sono salvati nel portachiavi di iOS su iPhone e nel portachiavi di sistema sul desktop (Portachiavi su macOS, Gestione credenziali su Windows, Secret Service su Linux, oppure un file leggibile solo dal tuo utente se non c&apos;è un portachiavi). Il player web li conserva nello storage locale del browser per il suo indirizzo. Al di fuori della sincronizzazione cifrata facoltativa di Connect descritta sotto, vengono inviati solo al server a cui appartengono. Le connessioni usano HTTPS per impostazione predefinita; HTTP semplice viene usato solo se lo consenti esplicitamente per un server locale di fiducia.</p>

        <h2>Dati conservati sul tuo dispositivo</h2>
        <p>Per costruire Ascolta, i mix di scoperta, gli album suggeriti e le statistiche di ascolto, Discophile registra ciò che ascolti su quel dispositivo, separatamente per ogni libreria. Gli ascolti giornalieri sono conservati per 400 giorni. Sul dispositivo sono salvati anche code, selezioni, chat con l&apos;assistente, musica e audiolibri scaricati e testi in cache.</p>
        <p>Su iPhone, cronologia di ascolto, statistiche, chat con l&apos;assistente e download sono salvati in file protetti esclusi dai backup del dispositivo. Le normali preferenze possono essere incluse nei backup, secondo le impostazioni del tuo dispositivo Apple. Sul desktop gli stessi dati si trovano nella cartella dati dell&apos;app per il tuo utente, indicata in Impostazioni, e sono inclusi nei backup che fai di quella cartella. Il player web li conserva nello storage del browser. Puoi consultare o cancellare la cronologia di ascolto in Impostazioni.</p>

        <h2>Discophile Connect</h2>
        <p>Su desktop, l’accesso condivide identità dell’account, immagine del profilo e piano con l’istanza Connect che scegli. Associando i computer fidati, le impostazioni delle connessioni selezionate, le preferenze facoltative, i backup e i messaggi di riproduzione vengono caricati cifrati con il tuo codice di recupero privato. L’istanza conserva dati cifrati e metadati del servizio, come dispositivi associati, date, dimensioni e piano; non riceve la chiave di decifratura o i file audio. Solo i computer associati possono decifrare questi dati. Conserva il codice in privato: l’accesso non permette di recuperare un codice perso.</p>
        <p>Da Connect puoi controllare e revocare gli accessi, esportare i dati cifrati o eliminare il vault cloud. La revoca di un dispositivo richiede la condivisione di un nuovo codice di recupero con i computer ancora fidati. Le nuove connessioni vengono condivise solo dopo averle selezionate. Questi servizi sono facoltativi e non sono ancora integrati nell’app iPhone o nel player web. Consulta l’<a href="https://connect.discophile.app/privacy">informativa privacy di Connect</a> per i dettagli su account e servizio.</p>

        <h2>Assistente musicale e AI facoltativa</h2>
        <p><b>Su questo iPhone</b> usa Apple Intelligence sui dispositivi supportati. Le richieste sono elaborate sul dispositivo, e Discophile non passa mai di sua iniziativa a un servizio cloud.</p>
        <p><b>Modello locale</b>, sul desktop e nel player web, usa un server compatibile con OpenAI che configuri tu, come Ollama o LM Studio. Le richieste vanno solo all&apos;indirizzo che inserisci.</p>
        <p><b>ChatGPT</b> è facoltativo, disponibile su iPhone e sul desktop, e funziona solo dopo che hai collegato il tuo account ChatGPT. Quando invii una richiesta con ChatGPT selezionato, Discophile invia a OpenAI il testo della richiesta e una parte limitata della conversazione recente, compresi i messaggi scritti prima nella stessa chat. Può inviare anche titolo, artista e album del brano in riproduzione, i nomi delle tue raccolte salvate e un breve riepilogo della selezione che stai modificando. Per scegliere i brani invia titolo, artista, album, genere, anno e durata di un massimo di 200 brani candidati, identificati da ID temporanei.</p>
        <p>Discophile non invia a OpenAI il tuo audio, gli indirizzi dei server, le credenziali dei server, gli identificativi privati della libreria o le registrazioni del microfono. Le richieste sono fatte con l&apos;archiviazione disattivata, ma OpenAI le elabora secondo i propri termini, la propria informativa sulla privacy e le impostazioni del tuo account, e questo non garantisce che nessun dato venga conservato. Puoi scollegare ChatGPT in Impostazioni in qualsiasi momento. L&apos;azione facoltativa che copia una richiesta e apre ChatGPT condivide solo ciò che scegli di incollare.</p>
        <p>Le modifiche che l&apos;assistente propone al tuo server, come modificare una playlist, vengono applicate solo dopo la tua conferma.</p>

        <p><b>Informazioni</b> scrive una panoramica di un artista, un album, un brano o un podcast quando la chiedi, usando l&apos;assistente selezionato e le fonti descritte più avanti tra i servizi online.</p>

        <h2>Dettatura, Siri e notifiche</h2>
        <p>Queste funzioni fanno parte dell&apos;app per iPhone. La dettatura usa il riconoscimento vocale sul dispositivo e richiede i permessi per microfono e riconoscimento vocale. L&apos;audio del microfono non viene salvato né inviato a OpenAI o allo sviluppatore. Le richieste a Siri sono gestite da Apple secondo le tue impostazioni di Siri, poi risolte nella libreria selezionata. Le notifiche sulle risposte dell&apos;assistente sono generate localmente sull&apos;iPhone; Discophile non usa un servizio di notifiche push remoto.</p>

        <h2>Servizi online che puoi scegliere di usare</h2>
        <p>Alcune funzioni contattano servizi pubblici solo quando le usi. Ciascuno riceve le informazioni di rete necessarie a rispondere, come il tuo indirizzo IP, ed è regolato dalla propria informativa sulla privacy.</p>
        <ul>
          <li><b>Testi online</b>: è disattivata per impostazione predefinita. Se la attivi e apri il testo di un brano che il tuo server non ha, Discophile invia a LRCLIB titolo, artista, album e durata.</li>
          <li><b>Ricerca e link dei podcast</b>: Discophile invia alla directory di Apple Podcasts solo il testo che cerchi o il link che incolli. Per il link di uno show Spotify, legge la pagina pubblica dello show per ricavarne il titolo.</li>
          <li><b>Correzione per le cuffie</b>: Discophile scarica da GitHub l&apos;elenco dei profili AutoEq e il profilo che scegli. Non invia alcun dato sulla libreria o sugli ascolti.</li>
          <li><b>Informazioni musicali online</b>: è disattivata per impostazione predefinita. Se la attivi, Informazioni invia a MusicBrainz e Wikipedia il nome, il titolo e l&apos;artista dell&apos;elemento che apri. L&apos;assistente riceve i metadati pubblici dell&apos;elemento e questi estratti, mai identificativi del server.</li>
          <li><b>Controllo aggiornamenti</b> (app desktop): all&apos;avvio chiede a GitHub l&apos;ultima versione di Discophile. La richiesta contiene la versione dell&apos;app e nient&apos;altro. Puoi disattivarlo in Impostazioni &gt; Informazioni.</li>
        </ul>
        <p>Le credenziali dei server non vengono mai inviate a questi servizi.</p>

        <h2>Agenti sul desktop</h2>
        <p>L&apos;app desktop può avviare un server Model Context Protocol per agenti come Claude. È disattivato per impostazione predefinita, è raggiungibile solo dal tuo computer e richiede un token che copi dalle Impostazioni. Un agente collegato può leggere i metadati e gli identificativi della tua libreria, gestire playlist e preferiti e controllare la riproduzione; Discophile ti chiede conferma prima che un agente tolga brani da una playlist o la elimini. Ciò che l&apos;agente invia al proprio fornitore di AI è regolato dall&apos;agente e da quel fornitore.</p>

        <h2>Il player web</h2>
        <p>Il player web è un software che ospiti tu, per esempio con Docker. Lo sviluppatore non gestisce un&apos;istanza pubblica e non riceve nulla dalla tua. Chi gestisce un&apos;istanza, e il suo proxy facoltativo verso i tuoi server, può vedere le richieste che vi passano, come qualsiasi server web.</p>

        <h2>Nessun tracciamento</h2>
        <p>Discophile non include SDK pubblicitari o di analisi, non vende dati personali e non ti traccia tra app e siti web. La modalità sport controlla solo la riproduzione musicale; non registra allenamenti e non legge dati sanitari.</p>

        <h2>Conservazione e scelte</h2>
        <p>Rimuovere una libreria in Impostazioni elimina dal dispositivo la connessione e i relativi download. Non cancella nulla dal tuo server. Scollegare ChatGPT rimuove le sue credenziali dal dispositivo; le richieste già inviate a OpenAI vanno gestite nel tuo account OpenAI.</p>
        <p>Su iPhone, eliminare l&apos;app rimuove i suoi dati locali. Gli elementi del portachiavi possono sopravvivere all&apos;app, quindi rimuovi le librerie e scollega ChatGPT prima di eliminarla se vuoi cancellare le credenziali salvate. Con «Rimuovi app», invece, i dati restano sul dispositivo. I backup del dispositivo si gestiscono nelle impostazioni Apple. Sul desktop, rimuovere l&apos;app non cancella la sua cartella dati né le voci del portachiavi: rimuovi prima le librerie e scollega gli account, poi elimina la cartella indicata in Impostazioni se vuoi cancellare tutto. Nel player web, cancellare i dati del sito dal browser li rimuove.</p>

        <h2 id="website">Questo sito web</h2>
        <p>Il sito di Discophile non contiene pubblicità e non ti traccia su altri siti. Conta le visite con Matomo, ospitato dallo sviluppatore, senza cookie e con indirizzi IP anonimizzati; i cookie di misurazione vengono impostati solo se li consenti. La <a href="/it/cookie-policy">cookie policy</a> elenca tutto ciò che il sito salva. Caratteri e contenuti multimediali sono serviti dal sito stesso. I link di download portano a GitHub, che fornisce i file; il sito legge l&apos;elenco delle versioni da GitHub dal proprio server, senza inviare nulla che ti riguardi. Il sito è ospitato su Vercel, che tratta informazioni tecniche come il tuo indirizzo IP per servire le pagine e proteggere il servizio. Quando richiedi un invito a TestFlight, il modulo invia allo sviluppatore via email nome, indirizzo email, i server selezionati e il messaggio facoltativo. Questi dati sono usati solo per invitarti alla beta e risponderti. Gli inviti a TestFlight sono consegnati da Apple, che gestisce poi la tua partecipazione secondo i propri termini e la propria informativa sulla privacy.</p>
        <p>Puoi chiedere in qualsiasi momento la cancellazione di questi dati scrivendo a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. Uscire dalla beta nell&apos;app TestFlight interrompe l&apos;invio di nuove build.</p>

        <h2 id="newsletter">Newsletter</h2>
        <p>Dopo un download, il sito propone una newsletter sulle nuove versioni e su cosa sta arrivando. Se ti iscrivi, il tuo indirizzo email e il momento del consenso vengono inviati a Emailchef (un servizio di email gestito da Dueclic S.r.l. in Italia), che conserva l&apos;elenco e invia la newsletter per conto dello sviluppatore. Prima ricevi un&apos;email per confermare l&apos;iscrizione. L&apos;indirizzo è usato solo per questa newsletter e non viene mai venduto né condiviso. Ogni newsletter contiene un link per annullare l&apos;iscrizione, e puoi anche chiedere la cancellazione del tuo indirizzo scrivendo a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>

        <h2>Richieste sulla privacy</h2>
        <p>Lo sviluppatore non riceve la tua libreria, la cronologia di ascolto o le credenziali, quindi la maggior parte dei dati si gestisce solo sui tuoi dispositivi o sui tuoi server. Se contatti l&apos;assistenza o richiedi un invito a TestFlight, il tuo indirizzo email e ciò che scrivi sono usati solo per rispondere e risolvere la richiesta. Non inserire password o token nelle richieste di assistenza.</p>
        <p>A seconda di dove vivi, puoi avere il diritto di accedere ai dati personali, rettificarli o cancellarli, opporti al trattamento o limitarlo, chiedere la portabilità, revocare il consenso e presentare reclamo all&apos;autorità di protezione dei dati, in Italia il Garante per la protezione dei dati personali. Per esercitare questi diritti sui dati in possesso dello sviluppatore, scrivi a <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. Per i dati in possesso del gestore del tuo server o di OpenAI, contattali direttamente.</p>

        <h2>Modifiche</h2>
        <p>Gli aggiornamenti a questa informativa saranno pubblicati su questa pagina con una nuova data, e riportati nelle versioni successive dell&apos;app.</p>
      </main>

      <SiteFooter locale="it" current="privacy" note="© 2026 Andrea Debernardi" />
    </>
  );
}
