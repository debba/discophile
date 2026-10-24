"use client";

import { createContext, useCallback, useContext, useId, useRef, useState, type FormEvent, type ReactNode } from "react";
import { trackEvent } from "@/lib/analytics";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { LIMITS, SERVER_OPTIONS, parseRequest, validateRequest, type FieldErrors } from "@/lib/testflight";

type Status = "idle" | "sending" | "sent";

const TestFlightContext = createContext<{ open: () => void; locale: Locale }>({ open: () => {}, locale: "en" });

export function TestFlightProvider({ locale, children }: { locale: Locale; children: ReactNode }) {
  const t = ui[locale].testflight;
  const dialogRef = useRef<HTMLDialogElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const [formKey, setFormKey] = useState(0);
  const [status, setStatus] = useState<Status>("idle");

  const open = useCallback(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    openerRef.current = document.activeElement as HTMLElement | null;
    dialog.showModal();
    trackEvent("TestFlight", "Open form");
    dialog.querySelector<HTMLElement>("input[name=name]")?.focus();
  }, []);

  const handleClose = useCallback(() => {
    openerRef.current?.focus();
    // A finished request starts over with an empty form next time.
    if (status === "sent") {
      setStatus("idle");
      setFormKey((key) => key + 1);
    }
  }, [status]);

  const close = useCallback(() => dialogRef.current?.close(), []);

  return (
    <TestFlightContext.Provider value={{ open, locale }}>
      {children}
      <dialog
        ref={dialogRef}
        className="testflight"
        aria-labelledby="testflight-title"
        aria-describedby={status === "sent" ? undefined : "testflight-intro"}
        onClose={handleClose}
      >
        <div className="testflight-head">
          <h2 id="testflight-title">{t.title}</h2>
          <button type="button" className="dialog-close" onClick={close}>
            <svg width="20" height="20" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" /></svg>
            <span className="sr-only">{t.close}</span>
          </button>
        </div>
        <TestFlightForm key={formKey} locale={locale} status={status} setStatus={setStatus} onDone={close} />
      </dialog>
    </TestFlightContext.Provider>
  );
}

export function TestFlightButton({ className = "button primary", children }: { className?: string; children?: ReactNode }) {
  const { open, locale } = useContext(TestFlightContext);
  return (
    <button type="button" className={className} aria-haspopup="dialog" onClick={open}>
      {children ?? ui[locale].testflight.button}
    </button>
  );
}

function TestFlightForm({ locale, status, setStatus, onDone }: { locale: Locale; status: Status; setStatus: (status: Status) => void; onDone: () => void }) {
  const t = ui[locale].testflight;
  const id = useId();
  const formRef = useRef<HTMLFormElement>(null);
  const doneRef = useRef<HTMLHeadingElement>(null);
  const [errors, setErrors] = useState<FieldErrors>({});
  const [formError, setFormError] = useState("");

  const fieldId = (name: string) => `${id}-${name}`;
  const describedBy = (name: keyof FieldErrors, hint?: string) =>
    [hint, errors[name] ? fieldId(`${name}-error`) : null].filter(Boolean).join(" ") || undefined;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (status === "sending") return;
    const form = event.currentTarget;
    const data = new FormData(form);
    const request = parseRequest({
      name: data.get("name"),
      email: data.get("email"),
      servers: data.getAll("servers"),
      notes: data.get("notes"),
      consent: data.get("consent") === "yes",
      locale,
    });

    const fieldErrors = validateRequest(request);
    setErrors(fieldErrors);
    setFormError("");
    if (Object.keys(fieldErrors).length) {
      requestAnimationFrame(() => focusFirstError(form, fieldErrors));
      return;
    }

    setStatus("sending");
    try {
      const response = await fetch("/api/testflight", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ ...request, website: data.get("website") }),
      });
      const result = await response.json().catch(() => ({}));
      if (!response.ok) {
        setStatus("idle");
        setErrors(result.fieldErrors ?? {});
        setFormError(result.error ?? ui[locale].errors.failed);
        requestAnimationFrame(() => {
          if (!focusFirstError(form, result.fieldErrors ?? {})) form.querySelector<HTMLElement>("[type=submit]")?.focus();
        });
        return;
      }
      setStatus("sent");
      trackEvent("TestFlight", "Request sent");
      requestAnimationFrame(() => doneRef.current?.focus());
    } catch {
      setStatus("idle");
      setFormError(t.offline);
    }
  }

  if (status === "sent") {
    return (
      <div className="testflight-done">
        <h3 ref={doneRef} tabIndex={-1}>{t.doneTitle}</h3>
        <p>{t.doneText}</p>
        <button type="button" className="button primary" onClick={onDone}>{t.done}</button>
      </div>
    );
  }

  return (
    <form ref={formRef} onSubmit={submit} noValidate>
      <p id="testflight-intro">{t.intro}</p>
      <p className="form-required-note">{t.requiredNote}</p>

      <div className="field">
        <label htmlFor={fieldId("name")}>{t.name} <Required /></label>
        <input
          id={fieldId("name")} name="name" type="text" autoComplete="name" required maxLength={LIMITS.name}
          aria-invalid={errors.name ? true : undefined} aria-describedby={describedBy("name")}
        />
        <FieldError id={fieldId("name-error")} message={errors.name} />
      </div>

      <div className="field">
        <label htmlFor={fieldId("email")}>{t.email} <Required /></label>
        <p className="hint" id={fieldId("email-hint")}>{t.emailHint}</p>
        <input
          id={fieldId("email")} name="email" type="email" autoComplete="email" inputMode="email" spellCheck={false} required maxLength={LIMITS.email}
          aria-invalid={errors.email ? true : undefined} aria-describedby={describedBy("email", fieldId("email-hint"))}
        />
        <FieldError id={fieldId("email-error")} message={errors.email} />
      </div>

      <fieldset className="field">
        <legend>{t.servers}</legend>
        <div className="choices">
          {SERVER_OPTIONS.map((option) => (
            <label key={option} className="choice">
              <input type="checkbox" name="servers" value={option} />
              <span>{t.serverLabels[option] ?? option}</span>
            </label>
          ))}
        </div>
      </fieldset>

      <div className="field">
        <label htmlFor={fieldId("notes")}>{t.notes}</label>
        <textarea id={fieldId("notes")} name="notes" rows={3} maxLength={LIMITS.notes} />
      </div>

      <div className="field honeypot" aria-hidden="true">
        <label htmlFor={fieldId("website")}>Website</label>
        <input id={fieldId("website")} name="website" type="text" tabIndex={-1} autoComplete="off" />
      </div>

      <div className="field">
        <label className="choice consent">
          <input
            type="checkbox" name="consent" value="yes" required
            aria-invalid={errors.consent ? true : undefined} aria-describedby={describedBy("consent")}
          />
          <span>
            {t.consentBefore}{locale === "en" ? " " : ""}
            <a href={`${localePath(locale, "/privacy")}#website`} target="_blank">{t.consentLink}<span className="sr-only"> {t.newTab}</span></a>. <Required />
          </span>
        </label>
        <FieldError id={fieldId("consent-error")} message={errors.consent} />
      </div>

      <div className="form-status" role="alert">{formError}</div>
      <div className="sr-only" role="status">{status === "sending" ? t.sendingStatus : ""}</div>

      <button type="submit" className="button primary" aria-disabled={status === "sending"}>
        {status === "sending" ? t.sending : t.submit}
      </button>
    </form>
  );
}

function focusFirstError(form: HTMLFormElement, errors: FieldErrors) {
  const first = (["name", "email", "consent"] as const).find((name) => errors[name]);
  if (!first) return false;
  form.querySelector<HTMLElement>(`[name="${first}"]`)?.focus();
  return true;
}

function Required() {
  return <span className="required" aria-hidden="true">*</span>;
}

function FieldError({ id, message }: { id: string; message?: string }) {
  if (!message) return null;
  return (
    <p className="field-error" id={id}>
      <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="10" fill="currentColor" /><path d="M12 7v6M12 16.5v.5" stroke="#fff" strokeWidth="2.4" strokeLinecap="round" /></svg>
      {message}
    </p>
  );
}
