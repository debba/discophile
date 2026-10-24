type Labels = { name: string; provider: string; purpose: string; expiry: string };

const LABELS: Record<"en" | "it", Labels> = {
  en: { name: "Name", provider: "Provider", purpose: "Purpose", expiry: "Expiry" },
  it: { name: "Nome", provider: "Fornitore", purpose: "Finalità", expiry: "Durata" },
};

export function PolicyTable({ locale, id, caption, rows }: { locale: "en" | "it"; id: string; caption: string; rows: string[][] }) {
  const l = LABELS[locale];
  return (
    <div className="table-scroll" role="region" aria-labelledby={`${id}-caption`} tabIndex={0}>
      <table>
        <caption id={`${id}-caption`}>{caption}</caption>
        <thead>
          <tr>
            <th scope="col">{l.name}</th>
            <th scope="col">{l.provider}</th>
            <th scope="col">{l.purpose}</th>
            <th scope="col">{l.expiry}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(([name, ...cells]) => (
            <tr key={name + cells[0]}>
              <th scope="row">{name.startsWith("None") || name.startsWith("Nessuno") ? <i>{name}</i> : <code>{name}</code>}</th>
              {cells.map((cell) => <td key={cell}>{cell}</td>)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
