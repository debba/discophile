export const PROVIDERS = [
  { name: "Navidrome", key: "NAVIDROME", upstream: "http://navidrome:4533" },
  { name: "Subsonic", key: "SUBSONIC", upstream: "http://subsonic:4040" },
  { name: "OpenSubsonic", key: "OPEN_SUBSONIC", upstream: "http://opensubsonic:4533" },
  { name: "Jellyfin", key: "JELLYFIN", upstream: "http://jellyfin:8096" },
  { name: "Audiobookshelf", key: "AUDIOBOOKSHELF", upstream: "http://audiobookshelf:80" },
];

export type HostingMode = "compose" | "helm";
export type ProxyServer = { id: string; provider: string; name: string; url: string; enabled: boolean };
export type HostingConfig = {
  imageTag: string;
  port: string;
  ingress: boolean;
  ingressClass: string;
  host: string;
  tls: boolean;
  tlsSecret: string;
  servers: ProxyServer[];
};

export const proxyName = (key: string) => key.toLowerCase().replaceAll("_", "-");

export function defaultServer(provider: typeof PROVIDERS[number], mode: HostingMode): ProxyServer {
  const url = new URL(provider.upstream);
  if (mode === "helm") url.hostname += ".media.svc.cluster.local";
  return { id: provider.key, provider: provider.name, name: proxyName(provider.key), url: url.href.replace(/\/$/, ""), enabled: true };
}

export function defaultHostingConfig(mode: HostingMode): HostingConfig {
  return {
    imageTag: mode === "compose" ? "latest" : "",
    port: "8080",
    ingress: true,
    ingressClass: "nginx",
    host: "music.example.com",
    tls: true,
    tlsSecret: "discophile-tls",
    servers: PROVIDERS.map((provider) => ({ ...defaultServer(provider, mode), enabled: false })),
  };
}

export type HostingError = "name" | "duplicate" | "url" | "port" | "tag" | "host" | "dns";

const dnsName = (name: string) => name.length <= 253 && name.split(".").every((label) => /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/.test(label));

export function hostingErrors(config: HostingConfig, mode: HostingMode): Record<string, HostingError> {
  const errors: Record<string, HostingError> = {};
  if ((mode === "compose" || config.imageTag) && !/^[\w][\w.-]{0,127}$/.test(config.imageTag)) errors.imageTag = "tag";
  if (mode === "compose" && (!/^\d+$/.test(config.port) || +config.port < 1 || +config.port > 65535)) errors.port = "port";
  if (mode === "helm" && config.ingress) {
    if (!dnsName(config.host) || /^\d+(\.\d+){3}$/.test(config.host)) errors.host = "host";
    if (config.ingressClass && !dnsName(config.ingressClass)) errors.ingressClass = "dns";
    if (config.tls && !dnsName(config.tlsSecret)) errors.tlsSecret = "dns";
  }
  const enabled = config.servers.filter((server) => server.enabled);
  for (const server of enabled) {
    if (!/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/.test(server.name)) errors[`${server.id}-name`] = "name";
    else if (enabled.some((other) => other.id !== server.id && other.name === server.name)) errors[`${server.id}-name`] = "duplicate";
    try {
      const url = new URL(server.url);
      // The container writes this address into an nginx directive without additional escaping.
      if (!/^https?:\/\//.test(server.url) || !url.hostname || url.username || url.password || url.search || url.hash || /[\x00-\x20\x7f\s\\"'{};$`<>?#]/.test(server.url)) throw new Error("url");
    } catch {
      errors[`${server.id}-url`] = "url";
    }
  }
  return errors;
}

// JSON string literals are also YAML double-quoted scalars.
const quote = (value: string) => JSON.stringify(value);

export function hostingYaml(config: HostingConfig, mode: HostingMode): string {
  const servers = config.servers.filter((server) => server.enabled);
  if (mode === "compose") {
    return `services:
  discophile:
    image: ${quote(`ghcr.io/debba/discophile-web:${config.imageTag}`)}
    restart: unless-stopped
    ports: [${quote(`${Number(config.port)}:8080`)}]${servers.length ? `
    environment:
${servers.map((server) => `      DISCOPHILE_PROXY_${server.name.toUpperCase().replaceAll("-", "_")}: ${quote(server.url)}`).join("\n")}` : ""}`;
  }
  return `${config.imageTag ? `image:
  tag: ${quote(config.imageTag)}

` : ""}ingress:
  enabled: ${config.ingress}${config.ingress ? `${config.ingressClass ? `
  className: ${quote(config.ingressClass)}` : ""}
  hosts:
    - host: ${quote(config.host)}
      paths: [{ path: /, pathType: Prefix }]${config.tls ? `
  tls:
    - secretName: ${quote(config.tlsSecret)}
      hosts: [${quote(config.host)}]` : ""}` : ""}

proxies:${servers.length ? `
${servers.map((server) => `  ${quote(server.name)}: ${quote(server.url)}`).join("\n")}` : " {}"}`;
}
