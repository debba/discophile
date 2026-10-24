import type { Metadata } from "next";
import { SiteFooter } from "@/components/SiteFooter";
import { alternates } from "@/lib/i18n";

const description = "How Discophile handles your servers, listening history, optional AI features and online services.";

export const metadata: Metadata = {
  title: "Privacy policy | Discophile",
  description,
  alternates: alternates("/privacy"),
  openGraph: { type: "article", url: "/privacy", title: "Discophile privacy policy", description },
};

export default function Privacy() {
  return (
    <>
      <main id="main" className="policy" tabIndex={-1}>
        <h1>Privacy policy</h1>
        <p className="updated">Last updated: October 22, 2026</p>

        <p className="policy-summary">Discophile has no account system and no server of its own. It connects your iPhone directly to the music and audiobook servers you choose. Your listening history stays on your iPhone, and the app contains no advertising, analytics or tracking.</p>

        <h2>Who is responsible</h2>
        <p>Discophile is developed by Andrea Debernardi. For privacy requests and support, write to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>

        <h2>Your servers and library</h2>
        <p>Discophile is a client for Subsonic and OpenSubsonic, Navidrome, Jellyfin and Audiobookshelf servers, and for internet radio stations you add. It sends requests directly to the addresses you enter, to browse your library, stream and download audio, load artwork and lyrics, manage playlists and favorites, and sync audiobook positions. The operator of each server decides what that server records, for example in its access logs.</p>
        <p>Passwords, API keys and access tokens are stored in the iOS Keychain. They are sent only to the server they belong to. Connections use HTTPS by default; plain HTTP is used only when you explicitly allow it for a trusted local server.</p>

        <h2>Data kept on your iPhone</h2>
        <p>To build Listen Now, discovery mixes, album suggestions and listening statistics, Discophile records what you play on this iPhone, separately for each library. Daily listening is kept for 400 days. Saved queues, selections, assistant chats, downloaded music and audiobooks, and cached lyrics are also stored on the device.</p>
        <p>Listening history, statistics, assistant chats and downloads are stored in protected files that are excluded from device backups. Ordinary preferences may be included in your backups according to your Apple device settings. You can review or clear listening history in Settings.</p>

        <h2>Music assistant and optional AI</h2>
        <p><b>On this iPhone</b> uses Apple Intelligence on supported devices. Requests are processed on the device, and Discophile never switches to a cloud service automatically.</p>
        <p><b>ChatGPT</b> is optional and works only after you connect your own ChatGPT account. When you send a request with ChatGPT selected, Discophile sends OpenAI the text of your request and a limited portion of the recent conversation, including messages written earlier in the same chat. It may also send the title, artist and album of the current track, the names of your saved collections and a short summary of the selection you are editing. To pick songs, it sends title, artist, album, genre, year and duration for up to 200 candidate songs, identified by temporary IDs.</p>
        <p>Discophile does not send OpenAI your audio, server addresses, server credentials, private library identifiers or microphone recordings. Requests are made with storage disabled, but OpenAI processes them under its own terms, privacy policy and account settings, and this is not a guarantee that no data is retained. You can disconnect ChatGPT in Settings at any time. The optional action that copies a request and opens ChatGPT shares only what you choose to paste there.</p>
        <p>Changes the assistant proposes to your server, such as editing a playlist, are applied only after you confirm them.</p>

        <h2>Dictation, Siri and notifications</h2>
        <p>Dictation uses on-device speech recognition and requires microphone and speech permissions. Microphone audio is not saved and is not sent to OpenAI or to the developer. Siri requests are handled by Apple according to your Siri settings, then resolved against your selected library. Notifications about assistant replies are generated locally on the iPhone; Discophile does not use a remote push service.</p>

        <h2>Online services you can choose to use</h2>
        <p>Some features contact public services only when you use them. Each receives the network information needed to answer, such as your IP address, and is governed by its own privacy policy.</p>
        <ul>
          <li><b>Online lyrics</b> is off by default. When enabled, opening lyrics for a song your server has no lyrics for sends its title, artist, album and duration to LRCLIB.</li>
          <li><b>Podcast search and links</b> send only your search text, or the link you paste, to the Apple Podcasts directory. For a Spotify show link, Discophile reads the public Spotify show page to find its title.</li>
          <li><b>Headphone correction</b> downloads the AutoEq profile list and the profile you choose from GitHub. No library or listening data is sent.</li>
        </ul>
        <p>Server credentials are never sent to these services.</p>

        <h2>No tracking</h2>
        <p>Discophile does not include advertising or analytics SDKs, does not sell personal data, and does not track you across apps and websites. Sport mode controls music playback only; it does not record workouts or read health data.</p>

        <h2>Retention and your choices</h2>
        <p>Removing a library in Settings removes its connection and its downloads from the iPhone. It does not delete anything from your server. Signing out of ChatGPT removes its credentials from the device; requests already sent to OpenAI must be managed in your OpenAI account.</p>
        <p>Deleting the app removes its local data. Keychain items can outlive the app, so remove your libraries and sign out of ChatGPT before deleting it if you want saved credentials gone. Offloading the app keeps its data. Manage device backups in your Apple settings.</p>

        <h2 id="website">This website</h2>
        <p>The Discophile website has no advertising and does not track you across other websites. It counts visits with Matomo, self-hosted by the developer, without cookies and with anonymised IP addresses; measurement cookies are set only if you allow them. The <a href="/cookie-policy">cookie policy</a> lists everything the site stores. Fonts and media are served from the site itself. The site is hosted on Vercel, which processes technical information such as your IP address to deliver pages and protect the service. When you request a TestFlight invitation, the form sends your name, email address, the servers you selected and your optional message to the developer by email. They are used only to invite you to the beta and to reply to you. TestFlight invitations are delivered by Apple, which then handles your participation under its own terms and privacy policy.</p>
        <p>You can ask for these details to be deleted at any time by writing to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. Removing yourself from the beta in the TestFlight app stops further builds.</p>

        <h2>Privacy requests</h2>
        <p>The developer does not receive your library, listening history or credentials, so most data can only be managed on your iPhone or on your own servers. If you contact support or request a TestFlight invitation, your email address and what you write are used only to answer and resolve your request. Do not include passwords or tokens in support requests.</p>
        <p>Depending on where you live, you may have the right to access, correct or erase personal data, object to or restrict its processing, request portability, withdraw consent, and complain to your data protection authority. To exercise these rights for data held by the developer, write to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. For data held by your server operator or OpenAI, contact them directly.</p>

        <h2>Changes</h2>
        <p>Updates to this policy will be published on this page with a new date, and reflected in later versions of the app.</p>
      </main>

      <SiteFooter locale="en" current="privacy" note="© 2026 Andrea Debernardi" />
    </>
  );
}
