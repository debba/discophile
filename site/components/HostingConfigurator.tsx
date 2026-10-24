"use client";

import { useId, useRef, useState } from "react";
import { CodeBlock, CopyCodeButton } from "@/components/CodeBlock";
import { ui, type Locale } from "@/lib/i18n";
import { defaultHostingConfig, defaultServer, hostingErrors, hostingYaml, PROVIDERS, type HostingConfig, type HostingMode, type ProxyServer } from "@/lib/selfHosting";

const COPY = {
  en: {
    customize: "Customize", close: "Close", intro: "Choose your servers and edit the settings. Preview and copy the file, or apply it to the example on this page.",
    deployment: "Player settings", tag: "Image version", tagHint: "Use latest or a release tag.", helmTagHint: "Leave empty to use the chart’s version.", port: "Host port",
    ingress: "Enable Ingress", ingressClass: "Ingress class", classHint: "Leave empty to use the cluster’s default class.", host: "Player domain", tls: "Enable HTTPS (TLS)", secret: "TLS secret name",
    servers: "Your servers", serverHint: "Include the servers you need. Addresses must be reachable from the player container; include any base path.",
    include: "Include", name: "Proxy name", nameHint: "Lowercase letters, digits and dashes. Each server needs a unique name.", address: "Server address", another: "Another server", add: "Add server", remove: "Remove",
    preview: "Your configuration", fix: "Correct the highlighted fields to preview and copy your configuration.", reviewErrors: "Review errors", reset: "Reset", apply: "Apply and close",
    added: "Server added.", removed: "Server removed.", resetDone: "Configuration reset. All library providers are disabled.",
    copyFailed: "Copy failed. Select the code and copy it manually.",
    errors: {
      name: "Use lowercase letters, digits and dashes; begin and end with a letter or digit.", duplicate: "Choose a unique proxy name.",
      url: "Enter an HTTP or HTTPS address without credentials, query parameters, fragments or special characters such as spaces, quotes and braces.",
      port: "Enter a port from 1 to 65535.", tag: "Use a valid image tag: letters, digits, dots, underscores and dashes (up to 128 characters).",
      host: "Enter a domain such as music.example.com, without a scheme, port or path.", dns: "Use lowercase letters, digits, dots and dashes; begin and end each part with a letter or digit.",
    },
  },
  it: {
    customize: "Personalizza", close: "Chiudi", intro: "Scegli i server e modifica le impostazioni. Visualizza e copia il file, oppure applicalo all’esempio in questa pagina.",
    deployment: "Impostazioni del player", tag: "Versione dell’immagine", tagHint: "Usa latest o il tag di una release.", helmTagHint: "Lascia vuoto per usare la versione del chart.", port: "Porta sull’host",
    ingress: "Abilita Ingress", ingressClass: "Classe Ingress", classHint: "Lascia vuoto per usare la classe predefinita del cluster.", host: "Dominio del player", tls: "Abilita HTTPS (TLS)", secret: "Nome del Secret TLS",
    servers: "I tuoi server", serverHint: "Includi i server che ti servono. Gli indirizzi devono essere raggiungibili dal container del player; includi l’eventuale percorso di base.",
    include: "Includi", name: "Nome del proxy", nameHint: "Lettere minuscole, cifre e trattini. Ogni server deve avere un nome distinto.", address: "Indirizzo del server", another: "Un altro server", add: "Aggiungi server", remove: "Rimuovi",
    preview: "La tua configurazione", fix: "Correggi i campi evidenziati per visualizzare e copiare la configurazione.", reviewErrors: "Rivedi gli errori", reset: "Ripristina", apply: "Applica e chiudi",
    added: "Server aggiunto.", removed: "Server rimosso.", resetDone: "Configurazione ripristinata. Tutti i provider di librerie sono disabilitati.",
    copyFailed: "Copia non riuscita. Seleziona il codice e copialo manualmente.",
    errors: {
      name: "Usa lettere minuscole, cifre e trattini; inizia e termina con una lettera o cifra.", duplicate: "Scegli un nome distinto per ogni proxy.",
      url: "Inserisci un indirizzo HTTP o HTTPS senza credenziali, parametri, frammenti o caratteri speciali come spazi, virgolette e parentesi graffe.",
      port: "Inserisci una porta da 1 a 65535.", tag: "Usa un tag valido: lettere, cifre, punti, underscore e trattini (fino a 128 caratteri).",
      host: "Inserisci un dominio come music.example.com, senza protocollo, porta o percorso.", dns: "Usa lettere minuscole, cifre, punti e trattini; inizia e termina ogni parte con una lettera o cifra.",
    },
  },
};

export function HostingConfigurator({ mode, locale }: { mode: HostingMode; locale: Locale }) {
  const c = COPY[locale];
  const t = ui[locale].download;
  const labels = { code: locale === "it" ? "Codice" : "Code", copy: t.copy, copied: t.copied, copyFailed: c.copyFailed };
  const file = mode === "compose" ? "compose.yaml" : "values.yaml";
  const id = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const openerRef = useRef<HTMLButtonElement>(null);
  const nextId = useRef(0);
  const [config, setConfig] = useState(() => defaultHostingConfig(mode));
  const [draft, setDraft] = useState(() => defaultHostingConfig(mode));
  const [newProvider, setNewProvider] = useState(PROVIDERS[0].key);
  const [notice, setNotice] = useState("");
  const errors = hostingErrors(draft, mode);
  const valid = Object.keys(errors).length === 0;

  function open() {
    setDraft(config);
    setNotice("");
    dialogRef.current?.showModal();
    dialogRef.current?.querySelector<HTMLElement>("input")?.focus();
  }

  function update<K extends keyof HostingConfig>(key: K, value: HostingConfig[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  function updateServer(serverId: string, patch: Partial<ProxyServer>) {
    setDraft((current) => ({ ...current, servers: current.servers.map((server) => server.id === serverId ? { ...server, ...patch } : server) }));
  }

  function addServer() {
    const provider = PROVIDERS.find((provider) => provider.key === newProvider)!;
    const server = defaultServer(provider, mode);
    let suffix = 2;
    let name = server.name;
    while (draft.servers.some((other) => other.name === name)) name = `${server.name}-${suffix++}`;
    const added = { ...server, id: `custom-${nextId.current++}`, name };
    update("servers", [...draft.servers, added]);
    setNotice(c.added);
    requestAnimationFrame(() => document.getElementById(`${id}-${added.id}-name`)?.focus());
  }

  function removeServer(serverId: string) {
    update("servers", draft.servers.filter((server) => server.id !== serverId));
    setNotice(c.removed);
    document.getElementById(`${id}-provider`)?.focus();
  }

  function focusFirstError() {
    dialogRef.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
  }

  const field = (key: string, label: string, value: string, onChange: (value: string) => void, hint?: string, numeric = false) => (
    <ConfigField key={key} id={`${id}-${key}`} label={label} value={value} onChange={onChange} hint={hint} numeric={numeric} error={errors[key] ? c.errors[errors[key]] : undefined} />
  );

  return (
    <>
      <CodeBlock code={hostingYaml(config, mode)} title={file} labels={labels} actions={
        <button ref={openerRef} type="button" className="copy-button customize-button" aria-haspopup="dialog" onClick={open}>
          {c.customize}<span className="sr-only"> {file}</span>
        </button>
      } />
      <dialog ref={dialogRef} className="sheet hosting-config" aria-labelledby={`${id}-title`} aria-describedby={`${id}-intro`} onClose={() => openerRef.current?.focus()}>
        <div className="sheet-head">
          <h2 id={`${id}-title`}>{c.customize} <span>{file}</span></h2>
          <button type="button" className="dialog-close" aria-label={c.close} onClick={() => dialogRef.current?.close()}>
            <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" /></svg>
          </button>
        </div>
        <div className="config-body">
          <p id={`${id}-intro`} className="config-intro">{c.intro}</p>
          <form id={`${id}-form`} className="config-fields" noValidate onSubmit={(event) => { event.preventDefault(); if (valid) { setConfig(draft); dialogRef.current?.close(); } else focusFirstError(); }}>
            <fieldset className="config-group">
              <legend>{c.deployment}</legend>
              {field("imageTag", c.tag, draft.imageTag, (value) => update("imageTag", value), mode === "compose" ? c.tagHint : c.helmTagHint)}
              {mode === "compose" ? field("port", c.port, draft.port, (value) => update("port", value), undefined, true) : <>
                <label className="choice"><input type="checkbox" checked={draft.ingress} onChange={(event) => update("ingress", event.target.checked)} />{c.ingress}</label>
                {draft.ingress && <div className="config-ingress">
                  {field("host", c.host, draft.host, (value) => update("host", value))}
                  {field("ingressClass", c.ingressClass, draft.ingressClass, (value) => update("ingressClass", value), c.classHint)}
                  <label className="choice"><input type="checkbox" checked={draft.tls} onChange={(event) => update("tls", event.target.checked)} />{c.tls}</label>
                  {draft.tls && field("tlsSecret", c.secret, draft.tlsSecret, (value) => update("tlsSecret", value))}
                </div>}
              </>}
            </fieldset>

            <fieldset className="config-group">
              <legend>{c.servers}</legend>
              <p className="hint">{c.serverHint}</p>
              <p className="hint">{c.nameHint}</p>
              <div className="config-servers">
                {draft.servers.map((server) => (
                  <fieldset key={server.id} className={`config-server${server.enabled ? "" : " is-excluded"}`}>
                    <legend>{server.provider}</legend>
                    <div className="config-server-bar">
                      <label className="choice"><input type="checkbox" checked={server.enabled} aria-label={`${c.include} ${server.provider} (${server.name})`} onChange={(event) => updateServer(server.id, { enabled: event.target.checked })} />{c.include}</label>
                      {server.id.startsWith("custom-") && <button type="button" className="link-button" aria-label={`${c.remove} ${server.provider} (${server.name})`} onClick={() => removeServer(server.id)}>{c.remove}</button>}
                    </div>
                    {server.enabled && <>
                      {field(`${server.id}-name`, c.name, server.name, (name) => updateServer(server.id, { name }))}
                      {field(`${server.id}-url`, c.address, server.url, (url) => updateServer(server.id, { url }))}
                    </>}
                  </fieldset>
                ))}
              </div>
              <div className="config-add">
                <label htmlFor={`${id}-provider`}>{c.another}</label>
                <div>
                  <select id={`${id}-provider`} value={newProvider} onChange={(event) => setNewProvider(event.target.value)}>{PROVIDERS.map((provider) => <option key={provider.key} value={provider.key}>{provider.name}</option>)}</select>
                  <button type="button" className="button" onClick={addServer}>{c.add}</button>
                </div>
              </div>
            </fieldset>
          </form>
          <div className="config-preview">
            <h3>{c.preview}</h3>
            {valid ? <CodeBlock code={hostingYaml(draft, mode)} title={file} labels={labels} /> : <p className="config-notice" role="status">{c.fix}</p>}
          </div>
        </div>
        <div className="config-footer">
          <button type="button" className="link-button config-reset" onClick={() => { setDraft(defaultHostingConfig(mode)); setNotice(c.resetDone); }}>{c.reset}</button>
          <div className="config-footer-actions">
            {!valid && <button type="button" className="link-button config-review" onClick={focusFirstError}>{c.reviewErrors} ({Object.keys(errors).length})</button>}
            <button type="submit" form={`${id}-form`} className="button" disabled={!valid}>{c.apply}</button>
            <CopyCodeButton code={valid ? hostingYaml(draft, mode) : ""} title={file} labels={labels} className="button primary" disabled={!valid} />
          </div>
        </div>
        <span className="sr-only" role="status">{notice}</span>
      </dialog>
    </>
  );
}

function ConfigField({ id, label, value, onChange, hint, error, numeric }: {
  id: string; label: string; value: string; onChange: (value: string) => void; hint?: string; error?: string; numeric?: boolean;
}) {
  const describedBy = [hint ? `${id}-hint` : "", error ? `${id}-error` : ""].filter(Boolean).join(" ") || undefined;
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {hint && <p className="hint" id={`${id}-hint`}>{hint}</p>}
      <input id={id} type="text" inputMode={numeric ? "numeric" : undefined} value={value} onChange={(event) => onChange(event.target.value)} autoComplete="off" autoCapitalize="none" spellCheck={false} aria-invalid={error ? true : undefined} aria-describedby={describedBy} />
      {error && <p className="field-error" id={`${id}-error`}>{error}</p>}
    </div>
  );
}
