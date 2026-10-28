import type { Metadata } from "next";
import { SiteFooter } from "@/components/SiteFooter";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "How Discophile for iPhone, Android, desktop and the web handles your servers, listening history, optional AI features and online services.";

export const metadata: Metadata = pageMetadata({
  locale: "en",
  path: "/privacy",
  title: "Privacy policy | Discophile",
  socialTitle: "Discophile privacy policy",
  description,
  type: "article",
  imageAlt: OG_ALT.en,
});

export default function Privacy() {
  return (
    <>
      <main id="main" className="policy" tabIndex={-1}>
        <h1>Privacy policy</h1>
        <p className="updated">Last updated: October 28, 2026</p>

        <p className="policy-summary">Discophile connects directly to the music and audiobook servers you choose. No account is required for listening. Discophile Connect is an optional account and encrypted service. The apps contain no advertising, analytics or tracking.</p>

        <p>This policy covers Discophile for iPhone and Android, Discophile for Mac, Windows and Linux (the desktop app), the web player you can host yourself, and this website. Unless a section says otherwise, it applies to all of them.</p>

        <h2>Who is responsible</h2>
        <p>Discophile is developed by Andrea Debernardi. For privacy requests and support, write to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>

        <h2>Your servers and library</h2>
        <p>Discophile is a client for Subsonic and OpenSubsonic, Navidrome, Jellyfin and Audiobookshelf servers, and for internet radio stations you add. It sends requests directly to the addresses you enter, to browse your library, stream and download audio, load artwork and lyrics, manage playlists and favorites, and sync audiobook positions. The operator of each server decides what that server records, for example in its access logs.</p>
        <p>Passwords, API keys and access tokens are stored in the iOS Keychain on iPhone, encrypted with an Android Keystore key on Android, and in the system keychain on the desktop (Keychain on macOS, Credential Manager on Windows, Secret Service on Linux, or a file only your user account can read when no keychain is available). The web player keeps them in the browser&apos;s local storage for its address. Outside the optional encrypted Connect sync described below, they are sent only to the server they belong to. Connections use HTTPS by default; plain HTTP is used only when you explicitly allow it for a trusted local server.</p>

        <h2>Data kept on your device</h2>
        <p>To build Listen Now, discovery mixes, album suggestions and listening statistics, Discophile records what you play on that device, separately for each library. Daily listening is kept for 400 days. Saved queues, selections, assistant chats, downloaded music and audiobooks, and cached lyrics are also stored on the device.</p>
        <p>On iPhone, listening history, statistics, assistant chats and downloads are stored in protected files that are excluded from device backups. Ordinary preferences may be included in your backups according to your Apple device settings. On Android, this data is stored in app-private storage and app backup is disabled. On the desktop, the same data lives in the app&apos;s data folder for your user account, shown in Settings, and is included in any backup you make of that folder. The web player keeps it in the browser&apos;s storage. You can review or clear listening history in Settings.</p>

        <h2>Discophile Connect</h2>
        <p>On desktop, iPhone and Android, signing in shares your account identity, profile picture and plan with the Connect instance you choose. Selected connections, optional preferences, backups and playback messages are encrypted. The instance stores ciphertext and service metadata such as paired devices, timestamps, storage sizes and your plan; it does not receive audio files. Recovery depends on the instance: with account-managed recovery, the operator can recover encryption keys, while personal recovery requires you to keep your recovery code private.</p>
        <p>You can review and revoke access, export encrypted data or delete your cloud vault from Connect. Revoking a device rotates the vault keys. New connections are shared only after you select them. These services are optional and are not integrated into the web player. See the <a href="https://connect.discophile.app/privacy">Connect privacy policy</a> for account and service details.</p>

        <h2>Music assistant and optional AI</h2>
        <p><b>On this iPhone</b> uses Apple Intelligence on supported devices. Requests are processed on the device, and Discophile never switches to a cloud service automatically.</p>
        <p><b>On-device Android model</b>: compatible phones can use Gemini Nano through ML Kit; availability depends on the device and remains under validation.</p>
        <p><b>Local model</b>, on the desktop and in the web player, uses an OpenAI-compatible server you configure, such as Ollama or LM Studio. Requests go only to the address you enter.</p>
        <p><b>ChatGPT</b> is optional, available on iPhone, Android and desktop, and works only after you connect your own ChatGPT account. When you send a request with ChatGPT selected, Discophile sends OpenAI the text of your request and a limited portion of the recent conversation, including messages written earlier in the same chat. It may also send the title, artist and album of the current track, the names of your saved collections and a short summary of the selection you are editing. To pick songs, it sends title, artist, album, genre, year and duration for up to 200 candidate songs, identified by temporary IDs.</p>
        <p>Discophile does not send OpenAI your audio, server addresses, server credentials, private library identifiers or microphone recordings. Requests are made with storage disabled, but OpenAI processes them under its own terms, privacy policy and account settings, and this is not a guarantee that no data is retained. You can disconnect ChatGPT in Settings at any time. The optional action that copies a request and opens ChatGPT shares only what you choose to paste there.</p>
        <p>Changes the assistant proposes to your server, such as editing a playlist, are applied only after you confirm them.</p>

        <p><b>About</b> writes an overview of an artist, album, song or podcast when you ask for one, using the assistant you selected and the sources described under online services below.</p>

        <h2>Dictation, Siri and notifications</h2>
        <p>On iPhone, dictation uses on-device speech recognition and requires microphone and speech permissions. Microphone audio is not saved and is not sent to OpenAI or to the developer. On Android, dictation depends on the speech recognition available on the device and its permissions. Siri requests are handled by Apple according to your Siri settings, then resolved against your selected library. Notifications about assistant replies are generated locally on the iPhone; Discophile does not use a remote push service.</p>

        <h2>Online services you can choose to use</h2>
        <p>Some features contact public services only when you use them. Each receives the network information needed to answer, such as your IP address, and is governed by its own privacy policy.</p>
        <ul>
          <li><b>Online lyrics</b> is off by default. When enabled, opening lyrics for a song your server has no lyrics for sends its title, artist, album and duration to LRCLIB.</li>
          <li><b>Podcast search and links</b> send only your search text, or the link you paste, to the Apple Podcasts directory. For a Spotify show link, Discophile reads the public Spotify show page to find its title.</li>
          <li><b>Headphone correction</b> downloads the AutoEq profile list and the profile you choose from GitHub. No library or listening data is sent.</li>
          <li><b>Online music information</b> is off by default. When enabled, About sends the name, title and artist of the item you open to MusicBrainz and Wikipedia. The assistant receives the item&apos;s public metadata and these excerpts, never server identifiers.</li>
          <li><b>Update check</b> (desktop app) asks GitHub for the latest Discophile release at launch. The request carries the app version and nothing else. Turn it off in Settings &gt; About.</li>
        </ul>
        <p>Server credentials are never sent to these services.</p>

        <h2>Agents on the desktop</h2>
        <p>The desktop app can run a Model Context Protocol server for agents such as Claude. It is off by default, listens only on your own computer and requires a token you copy from Settings. An agent you connect can read your library&apos;s metadata and identifiers, manage playlists and favorites and control playback; Discophile asks you before an agent removes songs from a playlist or deletes one. What the agent sends to its own AI provider is governed by that agent and provider.</p>

        <h2>The web player</h2>
        <p>The web player is software you host yourself, for example with Docker. The developer does not run a public instance and does not receive anything from yours. Whoever operates an instance, and its optional proxy to your servers, can see the requests that pass through it, like any web server.</p>

        <h2>No tracking</h2>
        <p>Discophile does not include advertising or analytics SDKs, does not sell personal data, and does not track you across apps and websites. Sport mode controls music playback only; it does not record workouts or read health data.</p>

        <h2>Retention and your choices</h2>
        <p>Removing a library in Settings removes its connection and its downloads from the device. It does not delete anything from your server. Signing out of ChatGPT removes its credentials from the device; requests already sent to OpenAI must be managed in your OpenAI account.</p>
        <p>On iPhone, deleting the app removes its local data. Keychain items can outlive the app, so remove your libraries and sign out of ChatGPT before deleting it if you want saved credentials gone. Offloading the app keeps its data. Manage device backups in your Apple settings. On Android, uninstalling the app or clearing its data removes local connections and downloads. On the desktop, removing the app does not delete its data folder or keychain entries; remove your libraries and sign out first, then delete the folder shown in Settings if you want everything gone. In the web player, clearing the site&apos;s data in your browser removes it.</p>

        <h2 id="website">This website</h2>
        <p>The Discophile website has no advertising and does not track you across other websites. It counts visits with Matomo, self-hosted by the developer, without cookies and with anonymised IP addresses; measurement cookies are set only if you allow them. The <a href="/cookie-policy">cookie policy</a> lists everything the site stores. Fonts and media are served from the site itself. Download links point to GitHub, which serves the files; the site reads the list of releases from GitHub on its own server, without sending anything about you. The site is hosted on Vercel, which processes technical information such as your IP address to deliver pages and protect the service. When you request a TestFlight invitation, the form sends your name, email address, the servers you selected and your optional message to the developer by email. They are used only to invite you to the beta and to reply to you. TestFlight invitations are delivered by Apple, which then handles your participation under its own terms and privacy policy.</p>
        <p>You can ask for these details to be deleted at any time by writing to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. Removing yourself from the beta in the TestFlight app stops further builds.</p>

        <h2 id="newsletter">Newsletter</h2>
        <p>After a download, the website offers a newsletter about new releases and what&apos;s coming next. If you subscribe, your email address and the time of your consent are sent to Emailchef (an email service operated by Dueclic S.r.l. in Italy), which stores the list and sends the newsletter on the developer&apos;s behalf. You first receive an email to confirm the subscription. Your address is used only for this newsletter and is never sold or shared. Every newsletter has an unsubscribe link, and you can also ask for your address to be deleted by writing to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>.</p>

        <h2>Privacy requests</h2>
        <p>The developer does not receive your library, listening history or credentials, so most data can only be managed on your devices or on your own servers. If you contact support or request a TestFlight invitation, your email address and what you write are used only to answer and resolve your request. Do not include passwords or tokens in support requests.</p>
        <p>Depending on where you live, you may have the right to access, correct or erase personal data, object to or restrict its processing, request portability, withdraw consent, and complain to your data protection authority. To exercise these rights for data held by the developer, write to <a href="mailto:andrea@debbaweb.it">andrea@debbaweb.it</a>. For data held by your server operator or OpenAI, contact them directly.</p>

        <h2>Changes</h2>
        <p>Updates to this policy will be published on this page with a new date, and reflected in later versions of the app.</p>
      </main>

      <SiteFooter locale="en" current="privacy" note="© 2026 Andrea Debernardi" />
    </>
  );
}
