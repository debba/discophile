import { isLocale, ui, type Locale } from "@/lib/i18n";

export const runtime = "nodejs";

const SIGNUP_URL = "https://app.emailchef.com/signupwl/7o22666s726q5s6964223n2237363736227q";
const FORM_ID = "7676";
const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

const WINDOW_MS = 10 * 60 * 1000;
const MAX_PER_WINDOW = 5;
// Best effort only: each serverless instance keeps its own counters.
const recentRequests = new Map<string, number[]>();

function isRateLimited(key: string) {
  const now = Date.now();
  const hits = (recentRequests.get(key) ?? []).filter((time) => now - time < WINDOW_MS);
  hits.push(now);
  recentRequests.set(key, hits);
  return hits.length > MAX_PER_WINDOW;
}

/** Signs an address up to the Discophile newsletter on Emailchef, which then asks it to confirm. */
export async function POST(request: Request) {
  let body: Record<string, unknown>;
  try {
    body = await request.json();
  } catch {
    return Response.json({ error: ui.en.errors.unreadable }, { status: 400 });
  }
  const locale: Locale = isLocale(body.locale) ? body.locale : "en";
  const t = ui[locale].newsletter;

  // Bots fill the hidden field; pretend success so they move on.
  if (typeof body.website === "string" && body.website.trim()) return Response.json({ ok: true });

  const ip = request.headers.get("x-forwarded-for")?.split(",")[0]?.trim() || "unknown";
  if (isRateLimited(ip)) return Response.json({ error: ui[locale].errors.rateLimited }, { status: 429 });

  const email = typeof body.email === "string" ? body.email.trim() : "";
  if (!email || email.length > 254 || !EMAIL_PATTERN.test(email)) {
    return Response.json({ error: t.invalid, field: "email" }, { status: 422 });
  }
  if (body.consent !== true) return Response.json({ error: t.consentRequired, field: "consent" }, { status: 422 });

  const form = new URLSearchParams({
    "field[-1]": email,
    "field[-4]": "1",
    form_id: FORM_ID,
    lang: "",
    referrer: "https://www.discophile.app/",
  });
  try {
    const response = await fetch(`${SIGNUP_URL}/${locale}`, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: form,
      signal: AbortSignal.timeout(15_000),
    });
    const page = (await response.text()).replace(/<style[\s\S]*?<\/style>|<script[\s\S]*?<\/script>/g, "");
    // Emailchef answers 200 either way; a failed signup says so on the page.
    if (!response.ok || /errore|error/i.test(page.replace(/<[^>]+>/g, " "))) {
      console.error("Newsletter signup refused", response.status);
      return Response.json({ error: t.failed }, { status: 502 });
    }
    return Response.json({ ok: true });
  } catch {
    return Response.json({ error: t.failed }, { status: 502 });
  }
}
