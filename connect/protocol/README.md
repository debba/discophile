# Discophile Connect protocol

The public contract between Discophile apps and any Connect instance: the official service at `https://connect.discophile.app` or an independent compatible server.

- [Version 1](v1.md): discovery, native sign-in, account, and plan entitlements.
- [Examples](examples.md): complete requests and responses.
- [Conformance checks](conformance/check.mjs): `node conformance/check.mjs https://connect.example.org`.

This folder is written to be published on its own. It does not refer to the official service's private source code, database, or hosting, and a compatible server needs nothing from the official service.

To build and run your own instance, start from the [guide](https://github.com/debba/discophile/blob/main/connect/README.md).

Status: draft. The documents are licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) and the conformance script under the MIT License, so anyone can build a compatible server; see [LICENSE.md](https://github.com/debba/discophile/blob/main/connect/LICENSE.md). Device pairing, encrypted synchronization, and playback handoff will be specified separately after cryptographic review.

The published copy lives in [`connect/protocol`](https://github.com/debba/discophile/tree/main/connect/protocol) of the public Discophile repository.
