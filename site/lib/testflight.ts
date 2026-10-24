import { isLocale, ui, type Locale } from "./i18n";

export const SERVER_OPTIONS = [
  "Navidrome",
  "Subsonic or OpenSubsonic",
  "Jellyfin",
  "Audiobookshelf",
  "Other",
] as const;

export const LIMITS = { name: 80, email: 254, notes: 1000 } as const;

export type TestFlightRequest = {
  name: string;
  email: string;
  servers: string[];
  notes: string;
  consent: boolean;
  locale: Locale;
};

export type FieldErrors = Partial<Record<"name" | "email" | "consent", string>>;

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function validateRequest(request: TestFlightRequest): FieldErrors {
  const messages = ui[request.locale].errors;
  const errors: FieldErrors = {};
  if (!request.name) errors.name = messages.name;
  else if (request.name.length > LIMITS.name) errors.name = messages.nameLength(LIMITS.name);

  if (!request.email) errors.email = messages.email;
  else if (request.email.length > LIMITS.email || !EMAIL_PATTERN.test(request.email)) {
    errors.email = messages.emailFormat;
  }

  if (!request.consent) errors.consent = messages.consent;
  return errors;
}

/** Normalizes untrusted input, such as a JSON body or form data, into a request. */
export function parseRequest(input: Record<string, unknown>): TestFlightRequest {
  const text = (value: unknown, max: number) => (typeof value === "string" ? value.trim().slice(0, max) : "");
  const servers = Array.isArray(input.servers) ? input.servers : [];
  return {
    name: text(input.name, LIMITS.name + 1).replace(/[\r\n]+/g, " "),
    email: text(input.email, LIMITS.email + 1),
    servers: SERVER_OPTIONS.filter((option) => servers.includes(option)),
    notes: text(input.notes, LIMITS.notes),
    consent: input.consent === true,
    locale: isLocale(input.locale) ? input.locale : "en",
  };
}
