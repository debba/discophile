import { NextResponse, type NextRequest } from "next/server";
import { LOCALE_COOKIE, localePath, preferredLocale } from "@/lib/i18n";

/**
 * Sends visitors of an English page to its Italian version when their browser prefers
 * Italian. A language picked in the switcher is stored in a cookie and always wins.
 */
export function proxy(request: NextRequest) {
  const chosen = request.cookies.get(LOCALE_COOKIE)?.value;
  const locale = chosen === "en" || chosen === "it" ? chosen : preferredLocale(request.headers.get("accept-language"));
  if (locale !== "it") return NextResponse.next();

  const url = request.nextUrl.clone();
  url.pathname = localePath("it", url.pathname);
  const response = NextResponse.redirect(url, 307);
  response.headers.set("Vary", "Accept-Language, Cookie");
  return response;
}

// English pages only; /it pages are never redirected, so an Italian link always opens in Italian.
export const config = { matcher: ["/", "/privacy", "/cookie-policy", "/video"] };
