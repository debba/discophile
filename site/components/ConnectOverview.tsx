import type { Locale } from "@/lib/i18n";

const COPY = {
  en: {
    title: "Your devices, in sync.",
    body: "Discophile Connect brings selected connections and preferences to your trusted desktop and mobile devices, encrypted for your Connect vault. Your music streams directly from your servers.",
    free: "Free", freeBody: "2 devices, one selected connection and shared preferences.",
    connectBody: "10 devices, all selected connections, encrypted backups, remote playback controls and Continue here.",
    supporterBody: "The same features as Connect, with a larger contribution to development.",
    connectPrice: "€2.99/month or €24.99/year", supporterPrice: "€4.99/month or €39.99/year",
    note: "Connect is optional and available on desktop, iPhone and Android. Listening and local connections stay free. Paid plans are activated by email: no checkout or automatic renewal.",
    action: "Explore Connect and plans", account: "Open Connect",
  },
  it: {
    title: "I tuoi dispositivi, sincronizzati.",
    body: "Discophile Connect porta le connessioni che scegli e le preferenze sui tuoi dispositivi desktop e mobili fidati, cifrate nel tuo vault Connect. La musica arriva direttamente dai tuoi server.",
    free: "Free", freeBody: "2 dispositivi, una connessione selezionata e preferenze condivise.",
    connectBody: "10 dispositivi, tutte le connessioni selezionate, backup cifrati, telecomando e Continua qui.",
    supporterBody: "Le stesse funzioni di Connect, con un contributo maggiore allo sviluppo.",
    connectPrice: "2,99 €/mese o 24,99 €/anno", supporterPrice: "4,99 €/mese o 39,99 €/anno",
    note: "Connect è facoltativo e disponibile su desktop, iPhone e Android. Ascolto e connessioni locali restano gratuiti. I piani a pagamento si attivano via email: nessun checkout o rinnovo automatico.",
    action: "Scopri Connect e i piani", account: "Apri Connect",
  },
};

export function ConnectOverview({ locale }: { locale: Locale }) {
  const c = COPY[locale];
  return (
    <section className="connect-overview" id="connect" aria-labelledby="connect-title">
      <p className="eyebrow">Discophile Connect</p>
      <h2 id="connect-title">{c.title}</h2>
      <p>{c.body}</p>
      <div className="connect-plans">
        <div><h3>{c.free}</h3><p className="connect-price">0 €</p><p>{c.freeBody}</p></div>
        <div><h3>Connect</h3><p className="connect-price">{c.connectPrice}</p><p>{c.connectBody}</p></div>
        <div><h3>Supporter</h3><p className="connect-price">{c.supporterPrice}</p><p>{c.supporterBody}</p></div>
      </div>
      <p className="fineprint">{c.note}</p>
      <div className="actions">
        <a className="button primary" href="https://connect.discophile.app/plans">{c.action}</a>
        <a className="button" href="https://connect.discophile.app">{c.account}</a>
      </div>
    </section>
  );
}
