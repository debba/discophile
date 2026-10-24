# Self-hosting the web player

The Discophile web player is the desktop app compiled to WebAssembly: the same interface, running in the browser.
It is a static site, so you can serve it from anywhere. The container image adds what a static host can't: an
optional same-origin proxy to your servers, so the browser can reach them without CORS settings or mixed-content
errors.

- Image: `ghcr.io/debba/discophile-web` (linux/amd64 and linux/arm64), an unprivileged nginx on port 8080.
- Helm chart: `oci://ghcr.io/debba/charts/discophile-web`.

Both are published with every [release](https://github.com/debba/discophile/releases), with the same
version number as the desktop app.

> **Compared with the desktop app**, the web player cannot download music for offline listening or sign in to
> ChatGPT (a local model works if it allows the player's address), keeps credentials in the browser's local
> storage, and plays only the formats your browser supports. Use it on devices you trust.

## Quick start

```sh
docker run -d --name discophile -p 8080:8080 ghcr.io/debba/discophile-web:latest
```

Open http://localhost:8080 and connect your server. Navidrome works as is (see [CORS](#cors-and-the-proxy)).

## Docker Compose

Next to Navidrome and Audiobookshelf, with both served through the player's own address:

```yaml
services:
  discophile:
    image: ghcr.io/debba/discophile-web:latest
    restart: unless-stopped
    ports: ["8080:8080"]
    environment:
      DISCOPHILE_PROXY_NAVIDROME: http://navidrome:4533
      DISCOPHILE_PROXY_AUDIOBOOKSHELF: http://audiobookshelf:80

  navidrome:
    image: deluan/navidrome:latest
    volumes: ["./navidrome:/data", "/path/to/music:/music:ro"]

  audiobookshelf:
    image: ghcr.io/advplyr/audiobookshelf:latest
    volumes: ["./abs/config:/config", "./abs/metadata:/metadata", "/path/to/audiobooks:/audiobooks:ro"]
```

In Discophile, add the libraries as `http://localhost:8080/proxy/navidrome` and
`http://localhost:8080/proxy/audiobookshelf`. Behind HTTPS, use your public address instead, for example
`https://music.example.com/proxy/navidrome`.

## CORS and the proxy

A web page can talk to another address only if that server allows it (CORS). The player runs in the browser, so
this applies to every request it makes to your servers:

| Server | Without the proxy |
| --- | --- |
| Navidrome | Works: Navidrome allows requests from any origin. |
| Subsonic and other OpenSubsonic servers | Depends on the server; use the proxy if sign-in fails. |
| Jellyfin | Depends on your Jellyfin and reverse proxy settings; use the proxy if sign-in fails. |
| Audiobookshelf | Use the proxy: Audiobookshelf doesn't send CORS headers by default. |
| Local AI model (Ollama, LM Studio) | Allow the player's address, for example `OLLAMA_ORIGINS=https://music.example.com`. |

Two more browser rules apply. A player served over HTTPS cannot reach `http://` servers (mixed content), and
the equalizer and ReplayGain need audio served with CORS headers; without them music still plays, unprocessed.

**The proxy avoids all of this.** Every environment variable `DISCOPHILE_PROXY_<NAME>=<address>` publishes that
server at `/proxy/<name>/` on the player's own address. The name is lowercased and underscores become dashes
(`DISCOPHILE_PROXY_MY_JELLYFIN` becomes `/proxy/my-jellyfin/`). The address must start with `http://` or `https://`
and be reachable from the container. Requests then go to the same origin as the page, so no CORS headers are
needed, HTTPS covers the whole connection, and streaming, seeking and the equalizer work.

The proxy forwards only to the addresses you configure. It passes `X-Forwarded-For`, `X-Forwarded-Proto`,
`X-Forwarded-Host` and `X-Forwarded-Prefix`, streams responses without buffering, and allows long-running
requests. The access log records paths without query strings, because Subsonic requests carry a reusable
token there.

## HTTPS

Put the container behind the reverse proxy you already use (Caddy, Traefik, nginx, a Kubernetes ingress) and
forward to port 8080. The player needs no special headers and lives at the root of its own host. The image
doesn't serve it from a sub-path such as `/discophile/`; for that, host the static files yourself, built with
`trunk build --release --public-url /discophile/`.

## Kubernetes with Helm

```sh
helm install discophile oci://ghcr.io/debba/charts/discophile-web \
  --namespace media --create-namespace \
  --set ingress.enabled=true \
  --set ingress.hosts[0].host=music.example.com \
  --set ingress.hosts[0].paths[0].path=/ \
  --set ingress.hosts[0].paths[0].pathType=Prefix \
  --set proxies.navidrome=http://navidrome.media.svc.cluster.local:4533
```

Or with a values file:

```yaml
# values.yaml
ingress:
  enabled: true
  className: nginx
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt
  hosts:
    - host: music.example.com
      paths:
        - path: /
          pathType: Prefix
  tls:
    - secretName: discophile-tls
      hosts: [music.example.com]

proxies:
  navidrome: http://navidrome.media.svc.cluster.local:4533
  audiobookshelf: http://audiobookshelf.media.svc.cluster.local
```

```sh
helm upgrade --install discophile oci://ghcr.io/debba/charts/discophile-web -n media -f values.yaml
helm test discophile -n media
```

Then add `https://music.example.com/proxy/navidrome` in Discophile. Changing `proxies` restarts the pods. If you
use the ingress-nginx controller and stream long audiobooks, consider raising
`nginx.ingress.kubernetes.io/proxy-read-timeout`.

### Values

| Key | Default | Description |
| --- | --- | --- |
| `replicaCount` | `1` | Pods to run. The player is stateless, so any number works. |
| `image.repository` | `ghcr.io/debba/discophile-web` | Image to deploy. |
| `image.tag` | chart `appVersion` | Image tag. |
| `image.pullPolicy` | `IfNotPresent` | Pull policy. |
| `imagePullSecrets` | `[]` | Secrets for a private registry. |
| `proxies` | `{}` | Map of name to address, published at `/proxy/<name>/`. Names use lowercase letters, digits and dashes. |
| `extraEnv` | `[]` | Extra environment variables for the container. |
| `service.type` | `ClusterIP` | Service type. |
| `service.port` | `80` | Service port (the container listens on 8080). |
| `service.annotations` | `{}` | Service annotations. |
| `ingress.enabled` | `false` | Create an Ingress. |
| `ingress.className` | `""` | Ingress class. |
| `ingress.annotations` | `{}` | Ingress annotations, for example for cert-manager. |
| `ingress.hosts` | `discophile.example.com` on `/` | Hosts and paths. |
| `ingress.tls` | `[]` | TLS secrets and hosts. |
| `resources` | 10m CPU, 16Mi requested; 128Mi limit | Container resources. |
| `podSecurityContext` | non-root user 101, `RuntimeDefault` seccomp | Pod security context. |
| `securityContext` | read-only root filesystem, no privilege escalation, all capabilities dropped | Container security context. |
| `podAnnotations`, `podLabels` | `{}` | Extra pod metadata. |
| `nodeSelector`, `tolerations`, `affinity` | empty | Scheduling. |
| `nameOverride`, `fullnameOverride` | `""` | Resource names. |

## Updating

The web player doesn't check for updates itself. Pull a newer tag (or `latest`) and recreate the container, or
run `helm upgrade` with the new chart version. Browsers fetch the new version on the next visit: the page is never
cached, and the application files have content-hashed names.

## Building the image yourself

```sh
docker build -t discophile-web --build-arg VERSION=dev .
docker run --rm -p 8080:8080 discophile-web
```

The build compiles the WebAssembly once, on the build machine's architecture, and copies it into nginx images for
each target platform, so multi-architecture builds stay fast:

```sh
docker buildx build --platform linux/amd64,linux/arm64 -t <registry>/discophile-web:<tag> --push .
```

The files served by the container (and by `trunk build --release` in `crates/app`) can also be hosted on any
static host. Serve `.wasm` files as `application/wasm`, and don't cache `index.html`.
