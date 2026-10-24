import type { Metadata } from "next";
import { CookiePreferencesButton } from "@/components/CookieConsent";
import { PolicyTable } from "@/components/PolicyTable";
import { SiteFooter } from "@/components/SiteFooter";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "How the Discophile website uses cookies and similar technologies, and how you can control them.";

export const metadata: Metadata = pageMetadata({
  locale: "en",
  path: "/cookie-policy",
  title: "Cookie policy | Discophile",
  socialTitle: "Discophile cookie policy",
  description,
  type: "article",
  imageAlt: OG_ALT.en,
});

export default function CookiePolicy() {
  return (
    <>
      <main id="main" className="policy" tabIndex={-1}>
        <h1>Cookie policy</h1>
        <p className="updated">Last updated: October 24, 2026</p>

        <p className="policy-summary">The Discophile website counts visits with Matomo, hosted by the developer, without cookies. Measurement cookies are set only if you allow them. There is no advertising, no marketing and nothing is shared with third parties.</p>

        <h2>What are cookies?</h2>
        <p>Cookies are small text files a website stores in your browser, so it can remember information across pages and visits. Similar technologies, such as local storage, work in the same way. This policy covers all of them.</p>

        <h2>How this website uses cookies</h2>
        <p>The site uses a minimal set of cookies and similar technologies, grouped into the categories below. Pages, images, fonts and the intro video are served from the site itself. The site opens in Italian when your browser prefers it, and remembers the language and appearance you pick. You can change your choice at any time with <b>Cookie preferences</b> at the bottom of every page.</p>

        <h2>Cookie categories</h2>
        <h3 className="policy-category">Necessary <span className="badge">Always active</span></h3>
        <p>These are required for the website to work and cannot be turned off. This category includes <b>cookieless page-view statistics</b> with <a href="https://matomo.org">Matomo</a>, self-hosted by the developer: no cookies are set, the IP address is anonymised before it is stored, and no data is shared with third parties. This processing is based on legitimate interest, to understand aggregate, anonymous traffic.</p>
        <PolicyTable
          locale="en"
          id="necessary"
          caption="Necessary cookies and similar technologies"
          rows={[
            ["discophile-cookie-consent", "www.discophile.app", "Stores your cookie choice", "Persistent (local storage)"],
            ["discophile-locale", "www.discophile.app", "Remembers the language you picked in the language switcher. Without it, the site follows your browser's language", "12 months"],
            ["discophile-theme", "www.discophile.app", "Remembers the appearance you picked, Light or Dark. Not stored for Automatic, which follows your device", "Persistent (local storage)"],
            ["None (cookieless)", "analytics.debbaweb.it", "Anonymised page-view statistics without persistent identifiers, plus counts of the download window being opened, the platform downloaded, the intro video, TestFlight requests, newsletter sign-ups and GitHub Sponsors links", "Session only"],
            ["None (cookieless)", "www.discophile.app", "Spam protection for the TestFlight request and newsletter forms: requests from your IP address are counted in server memory, never written to disk", "10 minutes"],
            ["None (cookieless)", "Vercel", "Hosting: delivers the pages and protects the service, which involves processing your IP address in server logs", "Set by Vercel's log retention"],
          ]}
        />

        <h3 className="policy-category">Measurement <span className="badge">Optional</span></h3>
        <p>If you allow measurement cookies, Matomo can recognise returning visitors and measure engagement over time. IP addresses are still anonymised and no data is shared with third parties. Without your permission these cookies are never set, and withdrawing it deletes them.</p>
        <PolicyTable
          locale="en"
          id="measurement"
          caption="Measurement cookies"
          rows={[
            ["_pk_id.*", "analytics.debbaweb.it", "Recognises a returning visitor across visits", "13 months"],
            ["_pk_ses.*", "analytics.debbaweb.it", "Groups the pages of a single visit", "30 minutes"],
            ["_pk_ref.*", "analytics.debbaweb.it", "Stores the website that referred the visit", "6 months"],
          ]}
        />

        <h3 className="policy-category">Marketing</h3>
        <p>There are no marketing cookies. If a service that needs them is ever added, this policy will be updated first, and the site will ask for your consent before setting them.</p>

        <h2>Forms</h2>
        <p>The TestFlight request form and the newsletter sign-up set no cookies. A TestFlight request is delivered to the developer by email. A newsletter sign-up is passed by this site&apos;s server to Emailchef, which then sends you a confirmation email; no Emailchef script or cookie is loaded on this site. Both are described in the <a href="/privacy#website">privacy policy</a>.</p>

        <h2>Downloads</h2>
        <p>The list of releases is read from GitHub by this site&apos;s server, so opening the download window doesn&apos;t contact GitHub from your browser. The files themselves are downloaded from GitHub when you choose one.</p>

        <h2>Links to other websites</h2>
        <p>Links to GitHub (downloads, source code and GitHub Sponsors), the Arch User Repository, the developer&apos;s other projects and Apple services lead to websites with their own cookie policies. Nothing is loaded from them until you follow a link or start a download.</p>

        <h2>Managing your preferences</h2>
        <p>Your choice is stored only in your browser and is never sent to a server. You can change it at any time, or clear this site&apos;s data in your browser to be asked again.</p>
        <p><CookiePreferencesButton locale="en" /></p>

        <h2>Contact</h2>
        <p>For questions about this cookie policy, write to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>
      </main>

      <SiteFooter locale="en" current="cookie-policy" note="© 2026 Andrea Debernardi" />
    </>
  );
}
