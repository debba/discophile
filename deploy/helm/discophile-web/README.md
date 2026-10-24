# discophile-web

[Discophile](https://discophile.app) is a player for Navidrome, Subsonic, Jellyfin and Audiobookshelf. This chart
deploys its web player: a stateless, unprivileged nginx serving the WebAssembly app, with optional same-origin
proxies to your servers so the browser needs no CORS settings.

```sh
helm install discophile oci://ghcr.io/debba/charts/discophile-web \
  --set proxies.navidrome=http://navidrome.media.svc.cluster.local:4533
```

Then add your library in Discophile as `https://<player address>/proxy/navidrome`.

The full guide, with every value, ingress and TLS examples, and how the proxy works, is in
[docs/WEB.md](https://github.com/debba/discophile/blob/main/docs/WEB.md).
