#!/bin/sh
# Turns each DISCOPHILE_PROXY_<NAME>=<url> into a same-origin proxy at /proxy/<name>/, so the browser reaches
# your servers without CORS or mixed content. Example: DISCOPHILE_PROXY_NAVIDROME=http://navidrome:4533
set -eu
out=/etc/nginx/discophile
rm -f "$out"/*.conf
if [ -f /proc/net/if_inet6 ]; then
    echo "listen [::]:8080;" > "$out/listen-ipv6.conf"
fi
env | grep '^DISCOPHILE_PROXY_' | while IFS='=' read -r key url; do
    name=$(printf '%s' "${key#DISCOPHILE_PROXY_}" | tr 'A-Z_' 'a-z-')
    url=${url%/}
    case "$name" in
        ''|*[!a-z0-9-]*) echo "discophile: ignoring $key: use letters, digits and underscores in the name" >&2; continue ;;
    esac
    case "$url" in
        http://?*|https://?*) ;;
        *) echo "discophile: ignoring $key: the address must start with http:// or https://" >&2; continue ;;
    esac
    cat > "$out/proxy-$name.conf" <<CONF
location /proxy/$name/ {
    proxy_pass $url/;
    proxy_http_version 1.1;
    proxy_ssl_server_name on;
    proxy_set_header Host \$proxy_host;
    proxy_set_header X-Real-IP \$remote_addr;
    proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto \$scheme;
    proxy_set_header X-Forwarded-Host \$host;
    proxy_set_header X-Forwarded-Prefix /proxy/$name;
    proxy_set_header Upgrade \$http_upgrade;
    proxy_set_header Connection \$connection_upgrade;
    proxy_buffering off;
    proxy_request_buffering off;
    proxy_read_timeout 1h;
    client_max_body_size 0;
}
CONF
    echo "discophile: /proxy/$name/ -> $url" >&2
done
