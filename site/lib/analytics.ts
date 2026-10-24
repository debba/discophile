// Matomo is self-hosted at analytics.debbaweb.it. It always starts cookieless and
// sets measurement cookies only after consent, so `_paq` can be pushed to at any time.
const MATOMO_URL = "https://analytics.debbaweb.it/";
const MATOMO_SITE_ID = "9";

export const CONSENT_KEY = "discophile-cookie-consent";
export const MANAGE_COOKIES_EVENT = "discophile:manage-cookies";

type Paq = unknown[][];
// Before matomo.js runs `_paq` is an array; afterwards Matomo swaps in an object with push().
type Queue = Paq | { push: (call: unknown[]) => void };
type MatomoWindow = Window & { _paq?: Queue; __matomoLoaded?: boolean; __matomoConsent?: boolean };

// Matomo warns when one of these is queued more than once before matomo.js runs.
const CONSENT_METHODS = new Set(["setCookieConsentGiven", "forgetCookieConsentGiven", "disableCookies"]);

function queue(): Queue {
  const w = window as MatomoWindow;
  return (w._paq = w._paq || []);
}

function consentCalls(cookieConsent: boolean): Paq {
  // forgetCookieConsentGiven also deletes measurement cookies set earlier.
  return cookieConsent ? [["setCookieConsentGiven"]] : [["forgetCookieConsentGiven"], ["disableCookies"]];
}

export function initMatomo(cookieConsent: boolean) {
  if (typeof window === "undefined") return;
  const w = window as MatomoWindow;
  if (w.__matomoConsent === cookieConsent) return;
  const _paq = queue();

  if (w.__matomoLoaded) {
    w.__matomoConsent = cookieConsent;
    if (Array.isArray(_paq)) {
      // matomo.js has not run yet: replace the queued consent calls instead of adding more.
      const rest = _paq.filter((call) => !CONSENT_METHODS.has(call[0] as string));
      _paq.splice(0, _paq.length, ...consentCalls(cookieConsent), ...rest);
    } else {
      consentCalls(cookieConsent).forEach((call) => _paq.push(call));
    }
    return;
  }
  w.__matomoLoaded = true;
  w.__matomoConsent = cookieConsent;

  _paq.push([cookieConsent ? "setCookieConsentGiven" : "disableCookies"]);
  _paq.push(["setTrackerUrl", MATOMO_URL + "matomo.php"]);
  _paq.push(["setSiteId", MATOMO_SITE_ID]);
  _paq.push(["trackPageView"]);
  _paq.push(["enableLinkTracking"]);

  const script = document.createElement("script");
  script.async = true;
  script.src = MATOMO_URL + "matomo.js";
  document.head.appendChild(script);
}

export function trackEvent(category: string, action: string, name?: string) {
  if (typeof window === "undefined") return;
  queue().push(name === undefined ? ["trackEvent", category, action] : ["trackEvent", category, action, name]);
}

export function manageCookies() {
  window.dispatchEvent(new Event(MANAGE_COOKIES_EVENT));
}
