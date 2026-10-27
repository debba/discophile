# Discophile Connect

Discophile Connect is an optional account for Discophile services. In this version, signing in shows your
account and plan in Discophile for computers. Compatible instances synchronize through the same account without exchanging codes;
Discophile works fully without an account: standalone use and direct connections to your own servers never
need one, and a plan never limits local playback.

The official instance is `https://connect.discophile.app`. You can use any other instance compatible with
version 1 of the Discophile Connect protocol, such as one you run yourself.

## First launch

The three introduction pages lead to **Choose how to get started**. **Skip** goes directly to that choice.
Choose **Sign in to Discophile Connect** or **Continue without an account**; either path continues to
connecting your libraries. Signing in later is always possible in Settings. **Back** returns to the last
introduction page and cancels a pending sign-in.

The official instance is selected by default. Expand **Use another Connect server** to enter a compatible
instance, choose **Use this address**, then sign in through your browser. The same address validation and
sign-in flow apply as in Settings.

## Choose the instance

Open **Settings → Discophile Connect**. The **Instance** section shows the instance in use: its name, when it
provides one, always together with its address.

To use another instance:

1. Type its address in **Custom address**, for example `https://connect.example.org`. Enter only the address:
   no path, query, user name or password. When you leave out `https://`, Discophile adds it.
2. Choose **Use this address**. Discophile reads the instance's description at `/api/v1/server-info` and
   switches only when it is a compatible instance that offers sign-in for Discophile for computers.

To go back, choose **Use the official instance**.

Discophile checks that the instance describes itself with the same address and that its sign-in, token and
account addresses belong to it. It never follows redirects to another address and always verifies HTTPS
certificates. Plain `http` is accepted only for `localhost`, `127.0.0.1` and `[::1]`, to develop a server on
the same computer; Settings then warns that the connection is not encrypted.

Changing the instance signs you out of the current one and revokes Discophile's access there. Nothing is sent
to the new instance: no tokens, identifiers or data. Sign in again to use it.

## Sign in

Signing in needs Discophile for computers; the web player cannot receive the sign-in redirect.

1. Choose the instance (the official one, or a custom address as above).
2. In **Account**, choose **Sign in to Discophile Connect**. Your browser opens the instance's sign-in page.
3. Sign in there and allow Discophile to read your account. The browser then returns to Discophile, which
   listens for the answer on `127.0.0.1` on this computer.

If the browser did not open, choose **Open the sign-in page again**. **Cancel** stops waiting, and an attempt
ends by itself after five minutes. A custom instance works the same way: the page you sign in on, and the ways
to sign in, are the instance's own.

Discophile uses OAuth with PKCE and asks for `account:read offline_access`, plus `connect:manage` when services are advertised. It never sees your password.

## What Discophile reads and stores

After signing in, Discophile reads your account name, email address and plan (its tier, how it was assigned,
its expiry date and the benefits it includes) and shows them in **Account**. **Refresh** reads them again, and
**Manage account** opens the instance's account page when it offers one.

Availability follows the instance capabilities and installed client version. Account-only instances continue to work; optional Supporter recognition and previews remain planned.

- Access tokens are stored only in this computer's keychain (or, when no keychain is available, in a file only
  you can read). Each sign-in belongs to one instance address and one account on it.
- The chosen instance address is saved with Discophile's other settings.
- With account synchronization, sign-in associates the computer and starts synchronization. Connections are selected automatically up to the plan allowance; change the selection in Settings. Payloads are encrypted, and Connect holds an encrypted copy of the keys so another signed-in computer can recover them. This mode trusts the instance with key custody; audio files are never uploaded.

Discophile contacts the instance when you choose it, sign in, refresh or sign out. Once paired, the running desktop app polls for services approximately every three seconds, with additional encrypted writes for shared changes and playback controls.

## Sign out and revoke access

In **Account**, choose **Sign out**. Discophile deletes its tokens from this computer first, then asks the
instance to revoke its access. If the instance cannot be reached, Settings says that revocation could not be
confirmed: you are still signed out on this computer, and you can remove Discophile's access from your account
page on the instance.

Changing the instance revokes access on the previous instance in the same way. If you remove Discophile's
access from the instance's account page yourself, Discophile notices the next time it reads your account and
asks you to sign in again.

## Pair computers and synchronize

In **Settings → Discophile Connect**, sign in and allow Connect services. Older account-only
authorizations need a new sign-in. On instances advertising `account-sync`, signing into the
same instance and account associates each computer automatically. No recovery code is required.
For an existing vault, open a previously connected computer once to migrate its key custody.
Older instances retain manual recovery under their existing pairing flow. Local connections
and downloads are preserved.

Choose the connections to share and whether to synchronize language and appearance. Volume,
effects, downloads, local paths and AI credentials stay on each computer. Selected Audiobookshelf
connections include the saved password inside the encrypted configuration. Each computer signs
in independently; refresh tokens are never copied. Older connections need one new sign-in on
the source computer to save the password. A different local Audiobookshelf account is preserved.
An HTTP or localhost server address may need adjusting on another computer.
Automatic synchronization detects concurrent changes and asks which shared configuration
to use. Removing a shared record never deletes another computer's local downloads.

Free includes 2 paired devices and 1 selected connection. Connect and Supporter include 10
devices, selected connections, encrypted configuration backups (up to 64 snapshots for 30 days),
remote controls and **Continue here**. Prices and manual email activation are shown under
**Plans and pricing**. No payment or automatic renewal is enabled.

## Remote control and Continue here

Paired computers poll while the app is open. A sleeping, closed or unreachable destination
cannot receive a transfer. Both computers need the same library configured and the destination
must reach its music server; Connect never relays audio. Use a paired device's Play, Pause,
Previous, Next, volume and seek controls. The player bar's **Playback devices** menu offers
**Play on this device** to send playback to another computer and **Continue here** to bring
its queue and position here. Queues are limited to 200 tracks. An authenticated Audiobookshelf
destination fetches the requested book even when it has never browsed it, preserving position,
speed and whether playback is paused.

The source keeps playing until the destination confirms that playback is ready. Failed or
expired transfers do not stop the source. A new playback action on the source invalidates
an old transfer acknowledgement. Volume stays specific to the destination computer.

While another device plays, the player bar shows its artwork, title and artist, and opens a
remote Now Playing with the timeline, transport, volume, **Continue here** and **Listen
independently here**. Presence carries an optional encrypted `media` object (`type` of
`music`, `podcast`, `radio` or `audiobook`, `title`, `artist`, `album`, `accountKey`, `itemId`,
`coverArt`). Artwork loads only when this computer is signed in to the same music library or
Audiobookshelf account, with its own credentials; nothing is fetched from the other device.
Devices that omit `media` still show the plain title.

## Recovery, revocation and backups

Account synchronization keeps an encrypted recovery bundle on Connect and restores it after
sign-in. The server can recover these keys: this is not end-to-end confidentiality from the
instance operator. Devices also retain keys in the system keychain or protected secrets file.
An optional private recovery export remains under **Advanced recovery**. Older instances
without account synchronization still require the owner's recovery code and cannot recover it.

Revoking a device blocks its OAuth delegation immediately. A remaining connected computer
rotates encryption, and the others retrieve the new keys through their account. Previously
downloaded data cannot be erased from a revoked computer. Legacy instances still need manual
distribution of updated recovery codes.

Backups contain configurations, not audio. Review a snapshot and restore it locally before
publishing it to other computers. Export and cloud deletion remain available on every tier,
including through the account dashboard when no trusted computer is available. Deleting
cloud data preserves local copies, libraries and downloads.

The [public service protocol](../connect/protocol/services.md) describes encrypted envelopes,
limits and compatibility. iPhone services are not enabled in this release.
