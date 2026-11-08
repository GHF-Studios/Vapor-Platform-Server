#!/usr/bin/env python3
"""Render the fixed Caddy templates with validated host names."""

from __future__ import annotations

import argparse
import ipaddress
import pathlib
import re
import sys


DNS_LABEL = re.compile(r"^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$", re.ASCII)
PLACEHOLDERS = {
    "{{VAPOR_DOMAIN}}",
    "{{VAPOR_ADMIN_DOMAIN_BLOCK}}",
    "{{VAPOR_PUBLIC_ADMIN_ENTRY_BLOCK}}",
    "{{VAPOR_PUBLIC_TERMINAL_BLOCK}}",
    "{{VAPOR_HTTP_FALLBACK_BLOCK}}",
}


def valid_dns_name(value: str) -> bool:
    if not value or len(value) > 253 or value.endswith("."):
        return False
    try:
        ipaddress.ip_address(value)
    except ValueError:
        pass
    else:
        return False
    labels = value.split(".")
    return len(labels) >= 2 and all(DNS_LABEL.fullmatch(label) for label in labels)


def terminal_route() -> str:
    return '''\tredir /emergency-terminal /emergency-terminal/ permanent
	# Root terminal is a separate service on a private Unix socket. Platform
	# checks Identity and the short grant on every resource/WS request.
	handle /emergency-terminal/* {
	\tlog_skip
	\theader Cache-Control "private, no-store"
	\tforward_auth 127.0.0.1:7110 {
	\t\turi /api/admin/emergency-console/authorize
	\t\theader_up X-Vapor-Console-Gateway CaddyEmergencyTerminalV1
	\t\theader_up X-Vapor-Console-Original-Method {http.request.method}
	\t\theader_up X-Vapor-Console-Original-Uri {http.request.uri}
	\t\theader_up X-Vapor-Console-Original-Origin {http.request.header.Origin}
	\t\theader_up X-Vapor-Console-Original-Upgrade {http.request.header.Upgrade}
	\t\theader_up X-Vapor-Console-Original-Fetch-Site {http.request.header.Sec-Fetch-Site}
	\t\tcopy_headers X-Vapor-Console-User
	\t}
	\treverse_proxy unix//run/vapor-emergency-console/ttyd.sock {
	\t\theader_up Host {http.request.host}
	\t\theader_up -Cookie
	\t\theader_up -Authorization
	\t\ttransport http {
	\t\t\tversions 1.1
	\t\t}
	\t\tstream_timeout 20m
	\t}
	}'''


def public_admin_entries(admin_domain: str | None, services: bool) -> str:
    platform = "127.0.0.1:7110"
    if not admin_domain:
        if services:
            return f'''\thandle /admin* {{
	\treverse_proxy {platform}
	}}

	handle /api/admin* {{
	\treverse_proxy {platform}
	}}

	handle /login* {{
	\treverse_proxy 127.0.0.1:7113
	}}

	handle /logout {{
	\treverse_proxy 127.0.0.1:7113
	}}

	handle /cli/ssh/* {{
	\treverse_proxy 127.0.0.1:7113
	}}'''
        return ""

    redirect = f"https://{admin_domain}{{http.request.uri}} 308"
    return f'''\thandle /admin* {{
	\tredir {redirect}
	}}

	handle /api/admin* {{
	\trespond "not found" 404
	}}

	handle /login* {{
	\tredir {redirect}
	}}

	handle /logout {{
	\tredir {redirect}
	}}

	handle /cli/ssh/* {{
	\tredir {redirect}
	}}

	handle /api/identity/v1/admin* {{
	\trespond "not found" 404
	}}'''


def public_terminal(admin_domain: str | None) -> str:
    if not admin_domain:
        return terminal_route()
    return '''\tredir /emergency-terminal /emergency-terminal/ permanent
	handle /emergency-terminal/* {
	\trespond "not found" 404
	}'''


def render(template_path: pathlib.Path, public_domain: str, admin_domain: str | None, fallback: str | None) -> str:
    if not valid_dns_name(public_domain):
        raise ValueError("public domain must be a DNS hostname")
    if admin_domain is not None:
        if not valid_dns_name(admin_domain) or admin_domain == public_domain:
            raise ValueError("admin domain must be a distinct DNS hostname")
    if fallback:
        try:
            ipaddress.ip_address(fallback)
        except ValueError:
            if not valid_dns_name(fallback):
                raise ValueError("HTTP fallback host must be an IP address or DNS hostname")

    text = template_path.read_text(encoding="utf-8")
    services = "services" in template_path.name
    admin_identity_routes = (
        '''\thandle /api/identity/v1/internal/* {
\t\trespond "not found" 404
\t}

\thandle_path /api/identity/* {
	\treverse_proxy 127.0.0.1:7113
	}

	handle /login* {
	\treverse_proxy 127.0.0.1:7113
	}

	handle /logout {
	\treverse_proxy 127.0.0.1:7113
	}

	handle /cli/ssh/* {
	\treverse_proxy 127.0.0.1:7113
	}'''
        if services
        else '''\thandle /api/identity/v1/internal/* {
\t\trespond "not found" 404
\t}

\thandle /api/identity/* {
	\treverse_proxy 127.0.0.1:7110
	}

	handle /login* {
	\treverse_proxy 127.0.0.1:7110
	}

	handle /logout {
	\treverse_proxy 127.0.0.1:7110
	}

	handle /cli/ssh/* {
	\treverse_proxy 127.0.0.1:7110
	}'''
    )
    public_admin = public_admin_entries(admin_domain, services)
    admin_block = ""
    if admin_domain:
        admin_block = f'''\n{admin_domain} {{
	import vapor_admin_routes
}}\n'''
    fallback_block = ""
    if fallback:
        fallback_block = f'''http://{fallback} {{
	redir https://{public_domain}{{http.request.uri}} permanent
}}
'''

    replacements = {
        "{{VAPOR_DOMAIN}}": public_domain,
        "{{VAPOR_ADMIN_DOMAIN_BLOCK}}": admin_block,
        "{{VAPOR_PUBLIC_ADMIN_ENTRY_BLOCK}}": public_admin,
        "{{VAPOR_PUBLIC_TERMINAL_BLOCK}}": public_terminal(admin_domain),
        "{{VAPOR_HTTP_FALLBACK_BLOCK}}": fallback_block,
        "{{VAPOR_ADMIN_IDENTITY_ROUTES}}": admin_identity_routes,
        "{{VAPOR_ADMIN_TERMINAL_ROUTE}}": terminal_route(),
    }
    for token, value in replacements.items():
        text = text.replace(token, value)
    unresolved = [token for token in (*PLACEHOLDERS, "{{VAPOR_ADMIN_IDENTITY_ROUTES}}", "{{VAPOR_ADMIN_TERMINAL_ROUTE}}") if token in text]
    if unresolved:
        raise ValueError("Caddy template contains unresolved placeholders")
    return text


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--template", required=True, type=pathlib.Path)
    parser.add_argument("--public-domain", required=True)
    parser.add_argument("--admin-domain")
    parser.add_argument("--http-fallback-host")
    args = parser.parse_args()
    try:
        rendered = render(args.template, args.public_domain, args.admin_domain, args.http_fallback_host)
    except (OSError, UnicodeError, ValueError) as error:
        print(f"caddy-render: {error}", file=sys.stderr)
        return 2
    sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
