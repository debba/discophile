# Discophile Connect

Discophile Connect is an optional account for Discophile services. In this version, signing in shows your
account and plan in Discophile for computers. **Nothing from your libraries is synchronized yet**, and
Discophile works fully without an account: standalone use and direct connections to your own servers never
need one, and a plan never limits local playback.

The official instance is `https://connect.discophile.app`. You can use any other instance compatible with
version 1 of the Discophile Connect protocol, such as one you run yourself.

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

Discophile uses OAuth with PKCE and asks for `account:read offline_access`. It never sees your password.

## What Discophile reads and stores

After signing in, Discophile reads your account name, email address and plan (its tier, how it was assigned,
its expiry date and the benefits it includes) and shows them in **Account**. **Refresh** reads them again, and
**Manage account** opens the instance's account page when it offers one.

Benefits that need features Discophile does not have yet, such as paired devices, encrypted synchronization,
configuration backups, remote control and playback handoff, are marked **Planned · not available yet** (or
**Not supported by this version of Discophile yet** when an instance already offers them). They are not
available in this version, whatever your plan.

- Access tokens are stored only in this computer's keychain (or, when no keychain is available, in a file only
  you can read). Each sign-in belongs to one instance address and one account on it.
- The chosen instance address is saved with Discophile's other settings.
- Nothing from your libraries, servers, credentials, listening history or playback is sent to the instance.

Discophile contacts the instance only when you choose it, sign in, refresh your account, or sign out, and at
start while you are signed in, to refresh your account.

## Sign out and revoke access

In **Account**, choose **Sign out**. Discophile deletes its tokens from this computer first, then asks the
instance to revoke its access. If the instance cannot be reached, Settings says that revocation could not be
confirmed: you are still signed out on this computer, and you can remove Discophile's access from your account
page on the instance.

Changing the instance revokes access on the previous instance in the same way. If you remove Discophile's
access from the instance's account page yourself, Discophile notices the next time it reads your account and
asks you to sign in again.
