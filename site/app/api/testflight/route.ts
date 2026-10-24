import nodemailer from "nodemailer";
import { ui } from "@/lib/i18n";
import { parseRequest, validateRequest } from "@/lib/testflight";

export const runtime = "nodejs";

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

function smtpConfig() {
  const { SMTP_HOST, SMTP_PORT, SMTP_SECURE, SMTP_USER, SMTP_PASS, SMTP_FROM, TESTFLIGHT_TO } = process.env;
  if (!SMTP_HOST || !SMTP_FROM || !TESTFLIGHT_TO) return null;
  const port = Number(SMTP_PORT || 587);
  return {
    transport: {
      host: SMTP_HOST,
      port,
      secure: SMTP_SECURE ? SMTP_SECURE === "true" : port === 465,
      auth: SMTP_USER ? { user: SMTP_USER, pass: SMTP_PASS ?? "" } : undefined,
    },
    from: SMTP_FROM,
    to: TESTFLIGHT_TO,
  };
}

export async function POST(request: Request) {
  let body: Record<string, unknown>;
  try {
    body = await request.json();
  } catch {
    return Response.json({ error: ui.en.errors.unreadable }, { status: 400 });
  }

  // Bots fill the hidden field; pretend success so they move on.
  if (typeof body.website === "string" && body.website.trim()) {
    return Response.json({ ok: true });
  }

  const testFlightRequest = parseRequest(body);
  const messages = ui[testFlightRequest.locale].errors;

  const ip = request.headers.get("x-forwarded-for")?.split(",")[0]?.trim() || "unknown";
  if (isRateLimited(ip)) {
    return Response.json({ error: messages.rateLimited }, { status: 429 });
  }

  const fieldErrors = validateRequest(testFlightRequest);
  if (Object.keys(fieldErrors).length > 0) {
    return Response.json({ error: messages.fields, fieldErrors }, { status: 422 });
  }

  const config = smtpConfig();
  if (!config) {
    console.error("TestFlight request rejected: SMTP is not configured");
    return Response.json({ error: messages.unavailable }, { status: 503 });
  }

  const { name, email, servers, notes, locale } = testFlightRequest;
  const text = [
    `Name: ${name}`,
    `Email: ${email}`,
    `Language: ${locale === "it" ? "Italian" : "English"}`,
    `Servers: ${servers.length ? servers.join(", ") : "Not specified"}`,
    "",
    "Notes:",
    notes || "None",
  ].join("\n");

  try {
    await nodemailer.createTransport(config.transport).sendMail({
      from: config.from,
      to: config.to,
      replyTo: { name, address: email },
      subject: `TestFlight request from ${name}`,
      text,
    });
  } catch (error) {
    console.error("TestFlight request could not be sent", error);
    return Response.json({ error: messages.failed }, { status: 502 });
  }

  return Response.json({ ok: true });
}
